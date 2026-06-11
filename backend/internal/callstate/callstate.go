package callstate

import (
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"sync"
	"time"

	"hopp-backend/internal/livekitutil"
	"hopp-backend/internal/messages"
	"hopp-backend/internal/redisutil"

	"github.com/labstack/echo/v4"
	"github.com/livekit/protocol/livekit"
	lksdk "github.com/livekit/server-sdk-go/v2"
	"github.com/redis/go-redis/v9"
	"github.com/twitchtv/twirp"
	"gorm.io/gorm"
)

const (
	snapshotKey       = "presence:snapshot"
	snapshotTTL       = 30 * time.Second
	pendingTTL        = 10 * time.Second
	reconcileInterval = 10 * time.Second
)

// Tracker owns the room-centric presence snapshot in Redis and reconciles it
// against LiveKit. A mutex serializes all snapshot reads and writes within a
// single process (sufficient for a single-instance deployment).
type Tracker struct {
	mu         sync.Mutex
	redis      *redis.Client
	db         *gorm.DB
	roomClient *lksdk.RoomServiceClient
	logger     echo.Logger
}

// NewTracker creates a presence tracker backed by the given Redis client.
// Call EnableReconciliation to wire up LiveKit before starting the loop.
func NewTracker(r *redis.Client, logger echo.Logger) *Tracker {
	return &Tracker{redis: r, logger: logger}
}

// EnableReconciliation wires up the LiveKit room service client and the DB used
// for resolving display names, enabling the reconciliation loop.
func (t *Tracker) EnableReconciliation(db *gorm.DB, serverURL, apiKey, secret string) error {
	httpURL, err := livekitutil.ConvertURLToHTTP(serverURL)
	if err != nil {
		return err
	}
	t.db = db
	t.roomClient = lksdk.NewRoomServiceClient(httpURL, apiKey, secret)
	return nil
}

// RoomPresence is a single validated room in the snapshot. A non-empty
// DisplayName => named/Slack room; empty => ad-hoc 1:1 call.
type RoomPresence struct {
	DisplayName string   `json:"displayName"`
	UserIDs     []string `json:"userIds"`
}

// Snapshot is the entire presence state, keyed by LiveKit room name.
type Snapshot struct {
	Rooms map[string]RoomPresence `json:"rooms"`
}

// CallPresence is the per-user projection returned to the API.
type CallPresence struct {
	PeerIDs  []string `json:"peerIds,omitempty"`
	RoomName string   `json:"roomName,omitempty"`
}

func pendingKey(roomName string) string {
	return fmt.Sprintf("presence:pending:%s", roomName)
}

func (t *Tracker) readSnapshot(ctx context.Context) (Snapshot, error) {
	snap := Snapshot{Rooms: map[string]RoomPresence{}}
	raw, err := t.redis.Get(ctx, snapshotKey).Result()
	if err != nil {
		if err == redis.Nil {
			return snap, nil
		}
		return snap, err
	}
	if err := json.Unmarshal([]byte(raw), &snap); err != nil {
		return Snapshot{Rooms: map[string]RoomPresence{}}, err
	}
	if snap.Rooms == nil {
		snap.Rooms = map[string]RoomPresence{}
	}
	return snap, nil
}

func (t *Tracker) writeSnapshot(ctx context.Context, snap Snapshot) error {
	data, err := json.Marshal(snap)
	if err != nil {
		return err
	}
	return t.redis.Set(ctx, snapshotKey, data, snapshotTTL).Err()
}

// setPending tries to claim the validation slot for a candidate room. The value
// is the LiveKit-listed members so the ACK handler can materialize without
// re-querying. Returns true if this caller won the SETNX race.
func (t *Tracker) setPending(ctx context.Context, roomName string, members []string) (bool, error) {
	data, err := json.Marshal(members)
	if err != nil {
		return false, err
	}
	return t.redis.SetNX(ctx, pendingKey(roomName), data, pendingTTL).Result()
}

// GetPending returns the members stashed for an in-flight candidate room.
func (t *Tracker) GetPending(ctx context.Context, roomName string) ([]string, error) {
	raw, err := t.redis.Get(ctx, pendingKey(roomName)).Result()
	if err != nil {
		if err == redis.Nil {
			return nil, nil
		}
		return nil, err
	}
	var members []string
	if err := json.Unmarshal([]byte(raw), &members); err != nil {
		return nil, err
	}
	return members, nil
}

// ClearPending removes the validation guard for a room.
func (t *Tracker) ClearPending(ctx context.Context, roomName string) error {
	return t.redis.Del(ctx, pendingKey(roomName)).Err()
}

// GetCallStates projects the room-centric snapshot into the per-user API shape
// for the requested users. The db param is unused (display names are resolved by
// the reconcile loop) but kept for call-site compatibility.
func (t *Tracker) GetCallStates(ctx context.Context, _ *gorm.DB, userIDs []string) (map[string]CallPresence, error) {
	result := make(map[string]CallPresence, len(userIDs))
	if len(userIDs) == 0 {
		return result, nil
	}
	t.mu.Lock()
	snap, err := t.readSnapshot(ctx)
	t.mu.Unlock()
	if err != nil {
		return result, err
	}
	want := make(map[string]bool, len(userIDs))
	for _, id := range userIDs {
		want[id] = true
	}
	for _, room := range snap.Rooms {
		for _, uid := range room.UserIDs {
			if !want[uid] {
				continue
			}
			peers := make([]string, 0, len(room.UserIDs)-1)
			for _, other := range room.UserIDs {
				if other != uid {
					peers = append(peers, other)
				}
			}
			result[uid] = CallPresence{PeerIDs: peers, RoomName: room.DisplayName}
		}
	}
	return result, nil
}

// RoomCount returns the number of active rooms in the snapshot.
func (t *Tracker) RoomCount(ctx context.Context) int {
	t.mu.Lock()
	snap, err := t.readSnapshot(ctx)
	t.mu.Unlock()
	if err != nil {
		return 0
	}
	return len(snap.Rooms)
}

// GetUserRoom returns the room name the user is currently in, if any.
func (t *Tracker) GetUserRoom(ctx context.Context, userID string) (string, bool, error) {
	t.mu.Lock()
	snap, err := t.readSnapshot(ctx)
	t.mu.Unlock()
	if err != nil {
		return "", false, err
	}
	for roomName, room := range snap.Rooms {
		if slices.Contains(room.UserIDs, userID) {
			return roomName, true, nil
		}
	}
	return "", false, nil
}

// AddCallRoom adds a validated room with the given members to the snapshot
// (used by explicit, client-driven events like accepting a call). Returns
// whether the snapshot changed.
func (t *Tracker) AddCallRoom(ctx context.Context, roomName string, userIDs []string, displayName string) (bool, error) {
	t.mu.Lock()
	defer t.mu.Unlock()

	snap, err := t.readSnapshot(ctx)
	if err != nil {
		return false, err
	}
	merged := mergeUserIDs(snap.Rooms[roomName].UserIDs, userIDs)
	next := RoomPresence{DisplayName: displayName, UserIDs: merged}
	if roomPresenceEqual(snap.Rooms[roomName], next) {
		return false, nil
	}
	snap.Rooms[roomName] = next
	return true, t.writeSnapshot(ctx, snap)
}

// AddUserToRoom adds a single user to a room, creating it if missing. The
// displayName is only applied when the room is created.
func (t *Tracker) AddUserToRoom(ctx context.Context, roomName, userID, displayName string) (bool, error) {
	t.mu.Lock()
	defer t.mu.Unlock()

	snap, err := t.readSnapshot(ctx)
	if err != nil {
		return false, err
	}
	existing, ok := snap.Rooms[roomName]
	if !ok {
		existing = RoomPresence{DisplayName: displayName}
	}
	merged := mergeUserIDs(existing.UserIDs, []string{userID})
	next := RoomPresence{DisplayName: existing.DisplayName, UserIDs: merged}
	if ok && roomPresenceEqual(existing, next) {
		return false, nil
	}
	snap.Rooms[roomName] = next
	return true, t.writeSnapshot(ctx, snap)
}

// MaterializeRoom adds a candidate room (validated via client ACK) to the
// snapshot, resolving its display name from the DB. Returns whether it changed.
func (t *Tracker) MaterializeRoom(ctx context.Context, roomName string, members []string) (bool, error) {
	if len(members) == 0 {
		return false, nil
	}
	t.mu.Lock()
	defer t.mu.Unlock()

	snap, err := t.readSnapshot(ctx)
	if err != nil {
		return false, err
	}
	displayName := snap.Rooms[roomName].DisplayName
	if displayName == "" {
		displayName = t.resolveDisplayName(roomName)
	}
	merged := mergeUserIDs(snap.Rooms[roomName].UserIDs, members)
	next := RoomPresence{DisplayName: displayName, UserIDs: merged}
	if roomPresenceEqual(snap.Rooms[roomName], next) {
		return false, nil
	}
	snap.Rooms[roomName] = next
	return true, t.writeSnapshot(ctx, snap)
}

// RemoveUser removes a user from whatever room they're in, collapsing the room
// if it becomes empty. Returns the room name, the remaining members, and whether
// the room is a named room (non-empty DisplayName => named/Slack room; empty =>
// ad-hoc 1:1 call).
func (t *Tracker) RemoveUser(ctx context.Context, userID string) (string, []string, bool, error) {
	t.mu.Lock()
	defer t.mu.Unlock()

	snap, err := t.readSnapshot(ctx)
	if err != nil {
		return "", nil, false, err
	}
	for roomName, room := range snap.Rooms {
		if !slices.Contains(room.UserIDs, userID) {
			continue
		}
		isNamedRoom := room.DisplayName != ""
		remaining := slices.DeleteFunc(slices.Clone(room.UserIDs), func(p string) bool {
			return p == userID
		})
		if len(remaining) == 0 {
			delete(snap.Rooms, roomName)
		} else {
			snap.Rooms[roomName] = RoomPresence{DisplayName: room.DisplayName, UserIDs: remaining}
		}
		return roomName, remaining, isNamedRoom, t.writeSnapshot(ctx, snap)
	}
	return "", nil, false, nil
}

// StartReconciliation launches the background loop that reconciles the snapshot
// against LiveKit every reconcileInterval. No-op if LiveKit is not configured.
func (t *Tracker) StartReconciliation() {
	if t.roomClient == nil {
		t.logger.Warnf("callstate: reconciliation disabled (no LiveKit client)")
		return
	}
	t.logger.Infof("callstate: starting presence reconciliation loop")
	go func() {
		ticker := time.NewTicker(reconcileInterval)
		defer ticker.Stop()
		for range ticker.C {
			ctx, cancel := context.WithTimeout(context.Background(), 9*time.Second)
			t.ReconcileOnce(ctx)
			cancel()
		}
	}()
}

// ReconcileOnce performs a single reconciliation pass:
//   - LiveKit is a hint: candidate rooms (not in snapshot) require a client ACK
//     before being added; validated rooms refresh membership from LiveKit.
//   - Rooms absent from a successful sweep are removed (recovers missed call-end).
//   - On a LiveKit API error the pass aborts without any removals.
func (t *Tracker) ReconcileOnce(ctx context.Context) {
	if t.roomClient == nil {
		return
	}

	roomsResp, err := t.roomClient.ListRooms(ctx, &livekit.ListRoomsRequest{})
	if err != nil {
		t.logger.Warnf("callstate: ListRooms failed, skipping reconcile tick: %v", err)
		return
	}

	// Live membership per room (only rooms with >=1 member). erroredRooms are
	// rooms whose participants couldn't be listed; we keep their snapshot entry.
	liveRooms := make(map[string][]string)
	erroredRooms := make(map[string]bool)
	for _, r := range roomsResp.Rooms {
		members, perr := t.listRoomMembers(ctx, r.Name)
		if perr != nil {
			t.logger.Warnf("callstate: ListParticipants failed for room %s: %v", r.Name, perr)
			erroredRooms[r.Name] = true
			continue
		}
		if len(members) > 0 {
			liveRooms[r.Name] = members
		}
	}

	// Lock only the snapshot read-modify-write; LiveKit API calls above are
	// intentionally outside the lock to avoid holding it during network I/O.
	t.mu.Lock()
	snap, err := t.readSnapshot(ctx)
	if err != nil {
		t.mu.Unlock()
		t.logger.Warnf("callstate: readSnapshot failed during reconcile: %v", err)
		return
	}

	next := Snapshot{Rooms: make(map[string]RoomPresence, len(snap.Rooms))}
	affected := make(map[string]bool)
	changed := false

	// Validated rooms: refresh from LiveKit. Candidate rooms: ping for ACK.
	for roomName, members := range liveRooms {
		existing, validated := snap.Rooms[roomName]
		if !validated {
			if acquired, perr := t.setPending(ctx, roomName, members); perr != nil {
				t.logger.Warnf("callstate: setPending failed for room %s: %v", roomName, perr)
			} else if acquired {
				for _, uid := range members {
					t.publishPresenceCheck(ctx, uid, roomName)
				}
			}
			continue
		}
		displayName := existing.DisplayName
		if displayName == "" {
			displayName = t.resolveDisplayName(roomName)
		}
		refreshed := RoomPresence{DisplayName: displayName, UserIDs: members}
		next.Rooms[roomName] = refreshed
		if !roomPresenceEqual(existing, refreshed) {
			changed = true
			markAffected(affected, existing.UserIDs)
			markAffected(affected, refreshed.UserIDs)
		}
	}

	// Snapshot rooms not present in liveRooms: keep if errored, else remove.
	for roomName, existing := range snap.Rooms {
		if _, live := liveRooms[roomName]; live {
			continue
		}
		if erroredRooms[roomName] {
			next.Rooms[roomName] = existing
			continue
		}
		changed = true
		markAffected(affected, existing.UserIDs)
	}

	if err := t.writeSnapshot(ctx, next); err != nil {
		t.mu.Unlock()
		t.logger.Warnf("callstate: writeSnapshot failed during reconcile: %v", err)
		return
	}
	t.mu.Unlock()

	if changed {
		for uid := range affected {
			t.publishPresenceChanged(ctx, uid)
		}
	}
}

// listRoomMembers returns the deduped user IDs in a LiveKit room. A not-found
// room is treated as empty.
func (t *Tracker) listRoomMembers(ctx context.Context, roomName string) ([]string, error) {
	participants, err := t.roomClient.ListParticipants(ctx, &livekit.ListParticipantsRequest{Room: roomName})
	if err != nil {
		if twErr, ok := err.(twirp.Error); ok && twErr.Code() == twirp.NotFound {
			return nil, nil
		}
		return nil, err
	}
	seen := make(map[string]bool)
	members := make([]string, 0, len(participants.Participants))
	for _, p := range participants.Participants {
		userID, err := livekitutil.ExtractUserIDFromIdentity(p.Identity)
		if err != nil {
			continue
		}
		if !seen[userID] {
			seen[userID] = true
			members = append(members, userID)
		}
	}
	return members, nil
}

func (t *Tracker) resolveDisplayName(roomName string) string {
	if t.db == nil {
		return ""
	}
	var name string
	err := t.db.Table("rooms").Select("name").Where("id = ?", roomName).Scan(&name).Error
	if err != nil {
		return ""
	}
	return name
}

func (t *Tracker) publishPresenceChanged(ctx context.Context, userID string) {
	data, err := json.Marshal(messages.NewPresenceChangedMessage())
	if err != nil {
		t.logger.Warnf("callstate: marshal presence_changed failed: %v", err)
		return
	}
	if err := t.redis.Publish(ctx, redisutil.GetUserChannel(userID), data).Err(); err != nil {
		t.logger.Warnf("callstate: publish presence_changed to %s failed: %v", userID, err)
	}
}

func (t *Tracker) publishPresenceCheck(ctx context.Context, userID, roomName string) {
	data, err := json.Marshal(messages.NewPresenceCheckMessage(roomName))
	if err != nil {
		t.logger.Warnf("callstate: marshal presence_check failed: %v", err)
		return
	}
	if err := t.redis.Publish(ctx, redisutil.GetUserChannel(userID), data).Err(); err != nil {
		t.logger.Warnf("callstate: publish presence_check to %s failed: %v", userID, err)
	}
}

func mergeUserIDs(existing, add []string) []string {
	out := slices.Clone(existing)
	for _, id := range add {
		if !slices.Contains(out, id) {
			out = append(out, id)
		}
	}
	return out
}

func roomPresenceEqual(a, b RoomPresence) bool {
	if a.DisplayName != b.DisplayName {
		return false
	}
	if len(a.UserIDs) != len(b.UserIDs) {
		return false
	}
	x := slices.Clone(a.UserIDs)
	y := slices.Clone(b.UserIDs)
	slices.Sort(x)
	slices.Sort(y)
	return slices.Equal(x, y)
}

func markAffected(set map[string]bool, userIDs []string) {
	for _, id := range userIDs {
		set[id] = true
	}
}
