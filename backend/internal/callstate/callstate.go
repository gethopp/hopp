package callstate

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"time"

	"github.com/redis/go-redis/v9"
	"gorm.io/gorm"
)

type Tracker struct {
	redis *redis.Client
}

func NewTracker(r *redis.Client) *Tracker {
	return &Tracker{redis: r}
}

func (t *Tracker) ResetAllCallState(ctx context.Context) error {
	patterns := []string{
		"user:call:*",
		"user:room:*",
		"lock:call:*",
		"call:pending:*",
	}
	for _, pattern := range patterns {
		var cursor uint64
		for {
			keys, next, err := t.redis.Scan(ctx, cursor, pattern, 100).Result()
			if err != nil {
				return fmt.Errorf("scan %q: %w", pattern, err)
			}
			if len(keys) > 0 {
				if err := t.redis.Del(ctx, keys...).Err(); err != nil {
					return fmt.Errorf("del %q keys: %w", pattern, err)
				}
			}
			cursor = next
			if cursor == 0 {
				break
			}
		}
	}
	return nil
}

// ErrCallEnded is returned when a call ends while a user is in the process of joining.
var ErrCallEnded = errors.New("call has ended")

const lockTTL = 3 * time.Second
const lockMaxRetries = 80 // ~4s total (80 * 50ms), exceeds lockTTL to ensure we wait for expiry

// acquireLock blocks until the lock is acquired or retries are exhausted.
func (t *Tracker) acquireLock(ctx context.Context, lockKey, holderID string) error {
	for range lockMaxRetries {
		acquired, err := t.redis.SetNX(ctx, lockKey, holderID, lockTTL).Result()
		if err != nil {
			return fmt.Errorf("failed to acquire call lock: %w", err)
		}
		if acquired {
			return nil
		}
		// Lock held by another goroutine — wait and retry
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(50 * time.Millisecond):
			// Retry
		}
	}
	return fmt.Errorf("timed out acquiring call lock after %d retries", lockMaxRetries)
}

type callEntry struct {
	Peers []string `json:"peers,omitempty"`
	Peer  string   `json:"peer,omitempty"` // legacy field for reading old Redis entries
	Room  string   `json:"room"`
}

func (e callEntry) GetPeers() []string {
	if len(e.Peers) > 0 {
		return e.Peers
	}
	if e.Peer != "" {
		return []string{e.Peer}
	}
	return nil
}

func callKey(userID string) string {
	return fmt.Sprintf("user:call:%s", userID)
}

func userRoomKey(userID string) string {
	return fmt.Sprintf("user:room:%s", userID)
}

func (t *Tracker) SetCallActive(ctx context.Context, userA, userB, roomName string) error {
	aVal, err := json.Marshal(callEntry{Peers: []string{userB}, Room: roomName})
	if err != nil {
		return err
	}
	bVal, err := json.Marshal(callEntry{Peers: []string{userA}, Room: roomName})
	if err != nil {
		return err
	}
	pipe := t.redis.Pipeline()
	pipe.Set(ctx, callKey(userA), aVal, 0)
	pipe.Set(ctx, callKey(userB), bVal, 0)
	_, err = pipe.Exec(ctx)
	return err
}

func (t *Tracker) RemoveCall(ctx context.Context, userA, userB string) error {
	pipe := t.redis.Pipeline()
	pipe.Del(ctx, callKey(userA))
	pipe.Del(ctx, callKey(userB))
	_, err := pipe.Exec(ctx)
	return err
}

func (t *Tracker) AddRoomParticipant(ctx context.Context, roomID, userID string) error {
	return t.redis.Set(ctx, userRoomKey(userID), roomID, 0).Err()
}

func (t *Tracker) RemoveRoomParticipant(ctx context.Context, roomID, userID string) error {
	return t.redis.Del(ctx, userRoomKey(userID)).Err()
}

// RemoveCallEntry deletes a single user's call entry (called when the call is fully over).
func (t *Tracker) RemoveCallEntry(ctx context.Context, userID string) error {
	return t.redis.Del(ctx, callKey(userID)).Err()
}

type CallPresence struct {
	InCall   bool     `json:"inCall"`
	PeerIDs  []string `json:"peerIds,omitempty"`
	RoomName string   `json:"roomName,omitempty"`
}

// GetCallStates fetches call/room presence for a batch of users in a single Redis round-trip.
func (t *Tracker) GetCallStates(ctx context.Context, db *gorm.DB, userIDs []string) (map[string]CallPresence, error) {
	if len(userIDs) == 0 {
		return map[string]CallPresence{}, nil
	}
	pipe := t.redis.Pipeline()
	callCmds := make([]*redis.StringCmd, len(userIDs))
	roomCmds := make([]*redis.StringCmd, len(userIDs))
	for i, id := range userIDs {
		callCmds[i] = pipe.Get(ctx, callKey(id))
		roomCmds[i] = pipe.Get(ctx, userRoomKey(id))
	}
	pipe.Exec(ctx) //nolint:errcheck — individual cmd errors checked below

	result := make(map[string]CallPresence, len(userIDs))
	for i, id := range userIDs {
		raw, err := callCmds[i].Result()
		if err == nil {
			var entry callEntry
			if json.Unmarshal([]byte(raw), &entry) == nil {
				result[id] = CallPresence{InCall: true, PeerIDs: entry.GetPeers()}
				continue
			}
		}
		roomID, err := roomCmds[i].Result()
		if err == nil && roomID != "" {
			roomName := roomID
			if db != nil {
				if room, lookupErr := lookupRoomName(db, roomID); lookupErr == nil && room != "" {
					roomName = room
				}
			}
			result[id] = CallPresence{InCall: true, RoomName: roomName}
		}
	}
	return result, nil
}

func lookupRoomName(db *gorm.DB, roomID string) (string, error) {
	var name string
	err := db.Table("rooms").Select("name").Where("id = ?", roomID).Scan(&name).Error
	return name, err
}

// CleanupUser removes all call/room state for a user (called on WS disconnect).
// Returns peer userIDs if the user was in a call.
// Acquires the room lock to avoid racing with JoinCall.
func (t *Tracker) CleanupUser(ctx context.Context, userID string) (callPeers []string, room string) {
	raw, getErr := t.redis.Get(ctx, callKey(userID)).Result()
	if getErr == nil {
		var entry callEntry
		if jsonErr := json.Unmarshal([]byte(raw), &entry); jsonErr == nil {
			callPeers = entry.GetPeers()

			// Acquire lock on the room before mutating peer entries
			if entry.Room != "" {
				lockKey := fmt.Sprintf("lock:call:%s", entry.Room)
				// Best-effort lock — don't block disconnect cleanup
				ctxLock, cancel := context.WithTimeout(ctx, 2*time.Second)
				if err := t.acquireLock(ctxLock, lockKey, userID); err != nil {
					// Fail open — don't block disconnect cleanup on lock contention
				} else {
					defer t.redis.Del(ctx, lockKey)
				}
				cancel()
			}

			pipe := t.redis.Pipeline()
			pipe.Del(ctx, callKey(userID))
			for _, peer := range callPeers {
				pipe.Del(ctx, callKey(peer))
			}
			pipe.Exec(ctx)
		}
	}

	room, _ = t.redis.GetDel(ctx, userRoomKey(userID)).Result()

	return callPeers, room
}

// JoinCall atomically checks if target is in a call and adds the joiner.
// Returns roomName and the list of existing participants (not including joiner).
// Returns empty roomName if target is not in a call.
func (t *Tracker) JoinCall(ctx context.Context, joinerID, targetUserID string) (roomName string, existingParticipants []string, err error) {
	// Phase 1: read target's entry to get room name
	raw, err := t.redis.Get(ctx, callKey(targetUserID)).Result()
	if err != nil {
		if err == redis.Nil {
			return "", nil, ErrCallEnded
		}
		return "", nil, err
	}
	var targetEntry callEntry
	if err := json.Unmarshal([]byte(raw), &targetEntry); err != nil {
		return "", nil, err
	}
	roomName = targetEntry.Room
	if roomName == "" {
		return "", nil, nil
	}

	// Phase 2: acquire lock on the room
	lockKey := fmt.Sprintf("lock:call:%s", roomName)
	if err := t.acquireLock(ctx, lockKey, joinerID); err != nil {
		return "", nil, err
	}
	defer t.redis.Del(ctx, lockKey)

	// Phase 3: re-read target's entry (may have been deleted between phase 1 and lock)
	raw, err = t.redis.Get(ctx, callKey(targetUserID)).Result()
	if err != nil {
		if err == redis.Nil {
			return "", nil, ErrCallEnded
		}
		return "", nil, err
	}
	if err := json.Unmarshal([]byte(raw), &targetEntry); err != nil {
		return "", nil, err
	}
	if targetEntry.Room != roomName {
		// Room changed or deleted while we were waiting for the lock
		return "", nil, ErrCallEnded
	}

	// Phase 4: add joiner under lock
	existingPeers := targetEntry.GetPeers()
	existingParticipants = append([]string{targetUserID}, existingPeers...)

	// Create joiner's entry with all existing participants
	joinerEntry, err := json.Marshal(callEntry{Peers: existingParticipants, Room: roomName})
	if err != nil {
		return "", nil, err
	}

	// Write joiner's entry + update each existing participant in one pipeline
	pipe := t.redis.Pipeline()
	pipe.Set(ctx, callKey(joinerID), joinerEntry, 0)
	for _, peerID := range existingParticipants {
		peerRaw, err := t.redis.Get(ctx, callKey(peerID)).Result()
		if err != nil {
			return "", nil, fmt.Errorf("failed to read peer %s entry: %w", peerID, err)
		}
		var peerEntry callEntry
		if err := json.Unmarshal([]byte(peerRaw), &peerEntry); err != nil {
			return "", nil, fmt.Errorf("failed to parse peer %s entry: %w", peerID, err)
		}
		// Avoid duplicates
		if !slices.Contains(peerEntry.Peers, joinerID) {
			peerEntry.Peers = append(peerEntry.Peers, joinerID)
		}
		peerVal, err := json.Marshal(peerEntry)
		if err != nil {
			return "", nil, err
		}
		pipe.Set(ctx, callKey(peerID), peerVal, 0)
	}

	if _, err = pipe.Exec(ctx); err != nil {
		return "", nil, err
	}

	return roomName, existingParticipants, nil
}

// LeaveCall removes a single user from a call.
// Returns the room name and remaining peer IDs.
// If the user was the last participant, returns empty peers and the call is fully cleaned up.
func (t *Tracker) LeaveCall(ctx context.Context, userID string) (roomName string, remainingPeers []string, err error) {
	// Phase 1: read user's entry to get room name and peers
	raw, err := t.redis.Get(ctx, callKey(userID)).Result()
	if err != nil {
		if err == redis.Nil {
			return "", nil, nil
		}
		return "", nil, err
	}
	var entry callEntry
	if err := json.Unmarshal([]byte(raw), &entry); err != nil {
		return "", nil, err
	}
	roomName = entry.Room
	if roomName == "" {
		return "", nil, nil
	}

	// Phase 2: acquire lock on the room
	lockKey := fmt.Sprintf("lock:call:%s", roomName)
	if err := t.acquireLock(ctx, lockKey, userID); err != nil {
		return "", nil, err
	}
	defer t.redis.Del(ctx, lockKey)

	// Phase 3: remove user and update peers
	peers := entry.GetPeers()
	pipe := t.redis.Pipeline()
	pipe.Del(ctx, callKey(userID))

	for _, peerID := range peers {
		peerRaw, err := t.redis.Get(ctx, callKey(peerID)).Result()
		if err != nil {
			// Peer already left, skip
			continue
		}
		var peerEntry callEntry
		if err := json.Unmarshal([]byte(peerRaw), &peerEntry); err != nil {
			continue
		}
		// Remove userID from peer's peer list
		peerEntry.Peers = slices.DeleteFunc(peerEntry.Peers, func(p string) bool {
			return p == userID
		})

		peerVal, marshalErr := json.Marshal(peerEntry)
		if marshalErr != nil {
			continue
		}
		pipe.Set(ctx, callKey(peerID), peerVal, 0)
	}

	if _, err = pipe.Exec(ctx); err != nil {
		return "", nil, err
	}

	return roomName, peers, nil
}
