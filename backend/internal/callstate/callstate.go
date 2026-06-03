package callstate

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/redis/go-redis/v9"
)

type Tracker struct {
	redis *redis.Client
}

func NewTracker(r *redis.Client) *Tracker {
	return &Tracker{redis: r}
}

type callEntry struct {
	Peer string `json:"peer"`
	Room string `json:"room"`
}

func callKey(userID string) string {
	return fmt.Sprintf("user:call:%s", userID)
}

func userRoomKey(userID string) string {
	return fmt.Sprintf("user:room:%s", userID)
}

func (t *Tracker) SetCallActive(ctx context.Context, userA, userB, roomName string) error {
	aVal, err := json.Marshal(callEntry{Peer: userB, Room: roomName})
	if err != nil {
		return err
	}
	bVal, err := json.Marshal(callEntry{Peer: userA, Room: roomName})
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

type CallPresence struct {
	InCall bool   `json:"inCall"`
	PeerID string `json:"peerId,omitempty"`
	RoomID string `json:"roomId,omitempty"`
}

// GetCallStates fetches call/room presence for a batch of users in a single Redis round-trip.
func (t *Tracker) GetCallStates(ctx context.Context, userIDs []string) (map[string]CallPresence, error) {
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
				result[id] = CallPresence{InCall: true, PeerID: entry.Peer}
				continue
			}
		}
		roomID, err := roomCmds[i].Result()
		if err == nil && roomID != "" {
			result[id] = CallPresence{InCall: true, RoomID: roomID}
		}
	}
	return result, nil
}

// CleanupUser removes all call/room state for a user (called on WS disconnect).
// Returns the peer's userID if the user was in a 1:1 call.
func (t *Tracker) CleanupUser(ctx context.Context, userID string) (callPeer string, room string) {
	raw, getErr := t.redis.Get(ctx, callKey(userID)).Result()
	if getErr == nil {
		var entry callEntry
		if jsonErr := json.Unmarshal([]byte(raw), &entry); jsonErr == nil {
			callPeer = entry.Peer
			pipe := t.redis.Pipeline()
			pipe.Del(ctx, callKey(userID))
			pipe.Del(ctx, callKey(entry.Peer))
			pipe.Exec(ctx)
		}
	}

	room, _ = t.redis.GetDel(ctx, userRoomKey(userID)).Result()

	return callPeer, room
}
