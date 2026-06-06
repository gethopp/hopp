package callstate

import (
	"context"
	"encoding/json"
	"sync"
	"testing"

	"github.com/alicebob/miniredis/v2"
	"github.com/redis/go-redis/v9"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
	"gorm.io/gorm/logger"
)

// setupTracker creates a Tracker backed by miniredis.
func setupTracker(t *testing.T) (*Tracker, *miniredis.Miniredis) {
	t.Helper()
	mr := miniredis.RunT(t)
	rdb := redis.NewClient(&redis.Options{Addr: mr.Addr()})
	t.Cleanup(func() { rdb.Close() })
	return NewTracker(rdb), mr
}

// setupDB creates an in-memory SQLite DB with a rooms table.
func setupDB(t *testing.T) *gorm.DB {
	t.Helper()
	db, err := gorm.Open(sqlite.Open(":memory:"), &gorm.Config{Logger: logger.Discard})
	require.NoError(t, err)
	require.NoError(t, db.Exec(`CREATE TABLE rooms (id TEXT PRIMARY KEY, name TEXT)`).Error)
	return db
}

func getCallEntry(t *testing.T, mr *miniredis.Miniredis, userID string) *callEntry {
	t.Helper()
	raw, err := mr.Get(callKey(userID))
	if err != nil {
		return nil
	}
	var e callEntry
	require.NoError(t, json.Unmarshal([]byte(raw), &e))
	return &e
}

func getRoomEntry(t *testing.T, mr *miniredis.Miniredis, userID string) string {
	t.Helper()
	v, err := mr.Get(userRoomKey(userID))
	if err != nil {
		return ""
	}
	return v
}

func assertNoCallKeys(t *testing.T, mr *miniredis.Miniredis) {
	t.Helper()
	for _, key := range mr.Keys() {
		if key[:5] == "user:" || key[:5] == "lock:" || key[:5] == "call:" {
			t.Errorf("unexpected key still present: %s", key)
		}
	}
}

func assertCallEntry(t *testing.T, mr *miniredis.Miniredis, userID string, peers []string, room string) {
	t.Helper()
	e := getCallEntry(t, mr, userID)
	require.NotNil(t, e, "call entry missing for %s", userID)
	assert.Equal(t, room, e.Room)
	assert.ElementsMatch(t, peers, e.Peers)
}

func assertSymmetricPeers(t *testing.T, mr *miniredis.Miniredis, userIDs []string) {
	t.Helper()
	for _, uid := range userIDs {
		e := getCallEntry(t, mr, uid)
		require.NotNil(t, e, "entry missing for %s", uid)
		for _, other := range userIDs {
			if other == uid {
				continue
			}
			assert.Contains(t, e.Peers, other, "%s should list %s as peer", uid, other)
		}
	}
}

func TestSetCallActive(t *testing.T) {
	tr, mr := setupTracker(t)
	ctx := context.Background()

	require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))

	assertCallEntry(t, mr, "a", []string{"b"}, "room1")
	assertCallEntry(t, mr, "b", []string{"a"}, "room1")
}

func TestRemoveCall(t *testing.T) {
	tr, mr := setupTracker(t)
	ctx := context.Background()

	require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
	require.NoError(t, tr.RemoveCallEntry(ctx, "a"))
	require.NoError(t, tr.RemoveCallEntry(ctx, "b"))

	assert.Nil(t, getCallEntry(t, mr, "a"))
	assert.Nil(t, getCallEntry(t, mr, "b"))

	// idempotent on missing keys
	require.NoError(t, tr.RemoveCallEntry(ctx, "a"))
}

func TestAddRoomParticipant(t *testing.T) {
	tr, mr := setupTracker(t)
	ctx := context.Background()

	require.NoError(t, tr.AddRoomParticipant(ctx, "room1", "u1"))
	assert.Equal(t, "room1", getRoomEntry(t, mr, "u1"))

	// Two users same room
	require.NoError(t, tr.AddRoomParticipant(ctx, "room1", "u2"))
	assert.Equal(t, "room1", getRoomEntry(t, mr, "u2"))

	// Overwrites previous room
	require.NoError(t, tr.AddRoomParticipant(ctx, "room2", "u1"))
	assert.Equal(t, "room2", getRoomEntry(t, mr, "u1"))
}

func TestRemoveRoomParticipant(t *testing.T) {
	tr, mr := setupTracker(t)
	ctx := context.Background()

	require.NoError(t, tr.AddRoomParticipant(ctx, "room1", "u1"))
	require.NoError(t, tr.RemoveRoomParticipant(ctx, "room1", "u1"))
	assert.Equal(t, "", getRoomEntry(t, mr, "u1"))

	// No error when key missing
	require.NoError(t, tr.RemoveRoomParticipant(ctx, "room1", "u1"))
}

func TestRoomTransition(t *testing.T) {
	tr, mr := setupTracker(t)
	ctx := context.Background()

	require.NoError(t, tr.AddRoomParticipant(ctx, "roomA", "u1"))
	require.NoError(t, tr.RemoveRoomParticipant(ctx, "roomA", "u1"))
	require.NoError(t, tr.AddRoomParticipant(ctx, "roomB", "u1"))

	assert.Equal(t, "roomB", getRoomEntry(t, mr, "u1"))
}

func TestJoinCall(t *testing.T) {
	ctx := context.Background()

	t.Run("three users", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		room, existing, err := tr.JoinCall(ctx, "c", "a")
		require.NoError(t, err)
		assert.Equal(t, "room1", room)
		assert.ElementsMatch(t, []string{"a", "b"}, existing)

		assertSymmetricPeers(t, mr, []string{"a", "b", "c"})
	})

	t.Run("target not in call", func(t *testing.T) {
		tr, _ := setupTracker(t)
		_, _, err := tr.JoinCall(ctx, "c", "ghost")
		assert.ErrorIs(t, err, ErrCallEnded)
	})
}

func TestLeaveCall(t *testing.T) {
	ctx := context.Background()

	t.Run("two-user call: both entries removed", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		room, peers, err := tr.LeaveCall(ctx, "a")
		require.NoError(t, err)
		assert.Equal(t, "room1", room)
		assert.ElementsMatch(t, []string{"b"}, peers)

		// Leaver deleted
		assert.Nil(t, getCallEntry(t, mr, "a"))
		// Last remaining user's entry also deleted (no orphan)
		assert.Nil(t, getCallEntry(t, mr, "b"), "orphaned single-user entry should be deleted")
	})

	t.Run("three-user call: remaining two are peers", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		_, _, err := tr.JoinCall(ctx, "c", "a")
		require.NoError(t, err)

		_, _, err = tr.LeaveCall(ctx, "a")
		require.NoError(t, err)

		assert.Nil(t, getCallEntry(t, mr, "a"))
		assertSymmetricPeers(t, mr, []string{"b", "c"})
	})

	t.Run("all leave sequentially: no keys remain", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		_, _, err := tr.JoinCall(ctx, "c", "a")
		require.NoError(t, err)

		tr.LeaveCall(ctx, "a") //nolint:errcheck
		tr.LeaveCall(ctx, "b") //nolint:errcheck
		tr.LeaveCall(ctx, "c") //nolint:errcheck

		assertNoCallKeys(t, mr)
	})

	t.Run("leave when not in call: no error", func(t *testing.T) {
		tr, _ := setupTracker(t)
		room, peers, err := tr.LeaveCall(ctx, "ghost")
		require.NoError(t, err)
		assert.Empty(t, room)
		assert.Empty(t, peers)
	})
}

func TestCleanupUser(t *testing.T) {
	ctx := context.Background()

	t.Run("cleans own entry, updates peers", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		peers, _ := tr.CleanupUser(ctx, "a")
		assert.ElementsMatch(t, []string{"b"}, peers)

		assert.Nil(t, getCallEntry(t, mr, "a"))
		// b had only a as peer, so b's entry should be deleted too
		assert.Nil(t, getCallEntry(t, mr, "b"), "orphaned entry should be deleted after cleanup")
	})

	t.Run("cleans room state", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.AddRoomParticipant(ctx, "room1", "u1"))
		_, room := tr.CleanupUser(ctx, "u1")
		assert.Equal(t, "room1", room)
		assert.Equal(t, "", getRoomEntry(t, mr, "u1"))
	})

	t.Run("noop for unknown user", func(t *testing.T) {
		tr, _ := setupTracker(t)
		peers, room := tr.CleanupUser(ctx, "ghost")
		assert.Empty(t, peers)
		assert.Empty(t, room)
	})

	t.Run("three-user: disconnect leaves remaining two as peers", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		_, _, err := tr.JoinCall(ctx, "c", "a")
		require.NoError(t, err)

		tr.CleanupUser(ctx, "a")

		assert.Nil(t, getCallEntry(t, mr, "a"))
		assertSymmetricPeers(t, mr, []string{"b", "c"})
	})
}

func TestGetCallStates(t *testing.T) {
	ctx := context.Background()

	t.Run("users in call", func(t *testing.T) {
		tr, _ := setupTracker(t)
		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))

		states, err := tr.GetCallStates(ctx, nil, []string{"a", "b"})
		require.NoError(t, err)
		require.True(t, states["a"].InCall)
		assert.ElementsMatch(t, []string{"b"}, states["a"].PeerIDs)
		require.True(t, states["b"].InCall)
		assert.ElementsMatch(t, []string{"a"}, states["b"].PeerIDs)
	})

	t.Run("user in room: resolved via db", func(t *testing.T) {
		tr, _ := setupTracker(t)
		db := setupDB(t)
		require.NoError(t, db.Exec(`INSERT INTO rooms (id, name) VALUES (?, ?)`, "r1", "My Room").Error)
		require.NoError(t, tr.AddRoomParticipant(ctx, "r1", "u1"))

		states, err := tr.GetCallStates(ctx, db, []string{"u1"})
		require.NoError(t, err)
		require.True(t, states["u1"].InCall)
		assert.Equal(t, "My Room", states["u1"].RoomName)
	})

	t.Run("idle user not in result", func(t *testing.T) {
		tr, _ := setupTracker(t)
		states, err := tr.GetCallStates(ctx, nil, []string{"nobody"})
		require.NoError(t, err)
		_, present := states["nobody"]
		assert.False(t, present)
	})

	t.Run("mixed batch", func(t *testing.T) {
		tr, _ := setupTracker(t)
		db := setupDB(t)
		require.NoError(t, db.Exec(`INSERT INTO rooms (id, name) VALUES (?, ?)`, "r1", "My Room").Error)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
		require.NoError(t, tr.AddRoomParticipant(ctx, "r1", "c"))

		states, err := tr.GetCallStates(ctx, db, []string{"a", "b", "c", "idle"})
		require.NoError(t, err)

		assert.True(t, states["a"].InCall)
		assert.True(t, states["b"].InCall)
		assert.True(t, states["c"].InCall)
		assert.Equal(t, "My Room", states["c"].RoomName)
		_, present := states["idle"]
		assert.False(t, present)
	})

	t.Run("empty input", func(t *testing.T) {
		tr, _ := setupTracker(t)
		states, err := tr.GetCallStates(ctx, nil, []string{})
		require.NoError(t, err)
		assert.Empty(t, states)
	})
}

func TestResetAllCallState(t *testing.T) {
	tr, mr := setupTracker(t)
	ctx := context.Background()

	require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))
	require.NoError(t, tr.AddRoomParticipant(ctx, "r1", "a"))

	require.NoError(t, tr.ResetAllCallState(ctx))
	assertNoCallKeys(t, mr)

	// Noop when empty
	require.NoError(t, tr.ResetAllCallState(ctx))
}

func TestConcurrentJoinLeave(t *testing.T) {
	ctx := context.Background()

	t.Run("concurrent joins: symmetric peers", func(t *testing.T) {
		tr, mr := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))

		var wg sync.WaitGroup
		joiners := []string{"c", "d", "e"}
		errs := make([]error, len(joiners))
		for i, joiner := range joiners {
			wg.Add(1)
			go func(idx int, id string) {
				defer wg.Done()
				_, _, errs[idx] = tr.JoinCall(ctx, id, "a")
			}(i, joiner)
		}
		wg.Wait()

		// All joiners must succeed
		for i, err := range errs {
			require.NoError(t, err, "joiner %s failed", joiners[i])
		}

		// Collect all participants
		all := append([]string{"a", "b"}, joiners...)
		assertSymmetricPeers(t, mr, all)
	})

	t.Run("concurrent join+leave: no corrupt state", func(t *testing.T) {
		tr, _ := setupTracker(t)

		require.NoError(t, tr.SetCallActive(ctx, "a", "b", "room1"))

		var wg sync.WaitGroup
		wg.Add(2)

		var joinErr, leaveErr error
		go func() {
			defer wg.Done()
			_, _, joinErr = tr.JoinCall(ctx, "c", "a")
		}()
		go func() {
			defer wg.Done()
			_, _, leaveErr = tr.LeaveCall(ctx, "a")
		}()
		wg.Wait()

		// Join either succeeds or returns ErrCallEnded; leave is always ok
		if joinErr != nil {
			assert.ErrorIs(t, joinErr, ErrCallEnded)
		}
		require.NoError(t, leaveErr)
	})
}
