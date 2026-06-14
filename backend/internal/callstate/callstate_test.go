package callstate

import (
	"context"
	"encoding/json"
	"sync"
	"testing"

	"github.com/alicebob/miniredis/v2"
	"github.com/labstack/echo/v4"
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
	return NewTracker(rdb, echo.New().Logger), mr
}

// setupDB creates an in-memory SQLite DB with a rooms table.
func setupDB(t *testing.T) *gorm.DB {
	t.Helper()
	db, err := gorm.Open(sqlite.Open(":memory:"), &gorm.Config{Logger: logger.Discard})
	require.NoError(t, err)
	sqlDB, _ := db.DB()
	sqlDB.SetMaxOpenConns(1)
	require.NoError(t, db.Exec(`CREATE TABLE rooms (id TEXT PRIMARY KEY, name TEXT)`).Error)
	return db
}

// readSnapshotRooms returns the snapshot rooms from Redis for assertions.
func readSnapshotRooms(t *testing.T, mr *miniredis.Miniredis) map[string]RoomPresence {
	t.Helper()
	raw, err := mr.Get(snapshotKey)
	if err != nil {
		return map[string]RoomPresence{}
	}
	var snap Snapshot
	require.NoError(t, json.Unmarshal([]byte(raw), &snap))
	if snap.Rooms == nil {
		return map[string]RoomPresence{}
	}
	return snap.Rooms
}

func TestAddCallRoom(t *testing.T) {
	ctx := context.Background()

	t.Run("creates room with members", func(t *testing.T) {
		tr, mr := setupTracker(t)

		changed, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)
		assert.True(t, changed)

		rooms := readSnapshotRooms(t, mr)
		require.Contains(t, rooms, "room1")
		assert.ElementsMatch(t, []string{"a", "b"}, rooms["room1"].UserIDs)
	})

	t.Run("sets display name", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a"}, "Team Standup")
		require.NoError(t, err)

		rooms := readSnapshotRooms(t, mr)
		assert.Equal(t, "Team Standup", rooms["room1"].DisplayName)
	})

	t.Run("merges members on repeat call", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		changed, err := tr.AddCallRoom(ctx, "room1", []string{"b", "c"}, "")
		require.NoError(t, err)
		assert.True(t, changed)

		rooms := readSnapshotRooms(t, mr)
		assert.ElementsMatch(t, []string{"a", "b", "c"}, rooms["room1"].UserIDs)
	})

	t.Run("idempotent when unchanged", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		changed, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)
		assert.False(t, changed)
	})

	t.Run("multiple rooms coexist", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)
		_, err = tr.AddCallRoom(ctx, "room2", []string{"c", "d"}, "Design")
		require.NoError(t, err)

		rooms := readSnapshotRooms(t, mr)
		require.Len(t, rooms, 2)
		assert.ElementsMatch(t, []string{"a", "b"}, rooms["room1"].UserIDs)
		assert.ElementsMatch(t, []string{"c", "d"}, rooms["room2"].UserIDs)
	})
}

func TestAddUserToRoom(t *testing.T) {
	ctx := context.Background()

	t.Run("creates room for first user", func(t *testing.T) {
		tr, mr := setupTracker(t)

		changed, err := tr.AddUserToRoom(ctx, "room1", "u1", "My Room")
		require.NoError(t, err)
		assert.True(t, changed)

		rooms := readSnapshotRooms(t, mr)
		require.Contains(t, rooms, "room1")
		assert.Equal(t, "My Room", rooms["room1"].DisplayName)
		assert.ElementsMatch(t, []string{"u1"}, rooms["room1"].UserIDs)
	})

	t.Run("appends user to existing room", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddUserToRoom(ctx, "room1", "u1", "My Room")
		require.NoError(t, err)

		changed, err := tr.AddUserToRoom(ctx, "room1", "u2", "Ignored Name")
		require.NoError(t, err)
		assert.True(t, changed)

		rooms := readSnapshotRooms(t, mr)
		assert.Equal(t, "My Room", rooms["room1"].DisplayName, "original display name preserved")
		assert.ElementsMatch(t, []string{"u1", "u2"}, rooms["room1"].UserIDs)
	})
}

func TestMaterializeRoom(t *testing.T) {
	ctx := context.Background()

	t.Run("resolves display name from DB", func(t *testing.T) {
		tr, mr := setupTracker(t)
		db := setupDB(t)
		tr.db = db
		require.NoError(t, db.Exec(`INSERT INTO rooms (id, name) VALUES (?, ?)`, "r1", "Design Sync").Error)

		changed, err := tr.MaterializeRoom(ctx, "r1", []string{"u1", "u2"})
		require.NoError(t, err)
		assert.True(t, changed)

		rooms := readSnapshotRooms(t, mr)
		assert.Equal(t, "Design Sync", rooms["r1"].DisplayName)
		assert.ElementsMatch(t, []string{"u1", "u2"}, rooms["r1"].UserIDs)
	})

	t.Run("preserves existing display name", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "r1", []string{"u1"}, "Already Named")
		require.NoError(t, err)

		changed, err := tr.MaterializeRoom(ctx, "r1", []string{"u2"})
		require.NoError(t, err)
		assert.True(t, changed)

		rooms := readSnapshotRooms(t, mr)
		assert.Equal(t, "Already Named", rooms["r1"].DisplayName)
		assert.ElementsMatch(t, []string{"u1", "u2"}, rooms["r1"].UserIDs)
	})

	t.Run("noop for empty members", func(t *testing.T) {
		tr, _ := setupTracker(t)

		changed, err := tr.MaterializeRoom(ctx, "r1", nil)
		require.NoError(t, err)
		assert.False(t, changed)
	})

	t.Run("idempotent when unchanged", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.MaterializeRoom(ctx, "r1", []string{"u1"})
		require.NoError(t, err)

		changed, err := tr.MaterializeRoom(ctx, "r1", []string{"u1"})
		require.NoError(t, err)
		assert.False(t, changed)
	})
}

func TestRemoveUser(t *testing.T) {
	ctx := context.Background()

	t.Run("removes user and returns remaining", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b", "c"}, "")
		require.NoError(t, err)

		roomName, remaining, _, err := tr.RemoveUser(ctx, "b")
		require.NoError(t, err)
		assert.Equal(t, "room1", roomName)
		assert.ElementsMatch(t, []string{"a", "c"}, remaining)

		rooms := readSnapshotRooms(t, mr)
		assert.ElementsMatch(t, []string{"a", "c"}, rooms["room1"].UserIDs)
	})

	t.Run("collapses empty room", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a"}, "")
		require.NoError(t, err)

		roomName, remaining, _, err := tr.RemoveUser(ctx, "a")
		require.NoError(t, err)
		assert.Equal(t, "room1", roomName)
		assert.Empty(t, remaining)

		rooms := readSnapshotRooms(t, mr)
		assert.NotContains(t, rooms, "room1")
	})

	t.Run("noop for unknown user", func(t *testing.T) {
		tr, _ := setupTracker(t)

		roomName, remaining, _, err := tr.RemoveUser(ctx, "ghost")
		require.NoError(t, err)
		assert.Empty(t, roomName)
		assert.Nil(t, remaining)
	})

	t.Run("does not affect other rooms", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)
		_, err = tr.AddCallRoom(ctx, "room2", []string{"c", "d"}, "")
		require.NoError(t, err)

		_, _, _, err = tr.RemoveUser(ctx, "a")
		require.NoError(t, err)

		rooms := readSnapshotRooms(t, mr)
		assert.ElementsMatch(t, []string{"b"}, rooms["room1"].UserIDs)
		assert.ElementsMatch(t, []string{"c", "d"}, rooms["room2"].UserIDs)
	})

	t.Run("sequential removal empties snapshot", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		_, _, _, err = tr.RemoveUser(ctx, "a")
		require.NoError(t, err)
		_, _, _, err = tr.RemoveUser(ctx, "b")
		require.NoError(t, err)

		rooms := readSnapshotRooms(t, mr)
		assert.Empty(t, rooms)
	})
}

func TestGetCallStates(t *testing.T) {
	ctx := context.Background()

	t.Run("returns peers for requested users", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b", "c"}, "Team")
		require.NoError(t, err)

		states, err := tr.GetCallStates(ctx, nil, []string{"a", "b"})
		require.NoError(t, err)

		require.Contains(t, states, "a")
		assert.ElementsMatch(t, []string{"b", "c"}, states["a"].PeerIDs)
		assert.Equal(t, "Team", states["a"].RoomName)

		require.Contains(t, states, "b")
		assert.ElementsMatch(t, []string{"a", "c"}, states["b"].PeerIDs)
	})

	t.Run("excludes unrequested users", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		states, err := tr.GetCallStates(ctx, nil, []string{"a"})
		require.NoError(t, err)

		assert.Contains(t, states, "a")
		assert.NotContains(t, states, "b")
	})

	t.Run("idle user absent from result", func(t *testing.T) {
		tr, _ := setupTracker(t)

		states, err := tr.GetCallStates(ctx, nil, []string{"nobody"})
		require.NoError(t, err)
		assert.NotContains(t, states, "nobody")
	})

	t.Run("empty input returns empty map", func(t *testing.T) {
		tr, _ := setupTracker(t)

		states, err := tr.GetCallStates(ctx, nil, nil)
		require.NoError(t, err)
		assert.Empty(t, states)
	})

	t.Run("users across different rooms", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "Room 1")
		require.NoError(t, err)
		_, err = tr.AddCallRoom(ctx, "room2", []string{"c", "d"}, "Room 2")
		require.NoError(t, err)

		states, err := tr.GetCallStates(ctx, nil, []string{"a", "c"})
		require.NoError(t, err)

		require.Contains(t, states, "a")
		assert.ElementsMatch(t, []string{"b"}, states["a"].PeerIDs)
		assert.Equal(t, "Room 1", states["a"].RoomName)

		require.Contains(t, states, "c")
		assert.ElementsMatch(t, []string{"d"}, states["c"].PeerIDs)
		assert.Equal(t, "Room 2", states["c"].RoomName)
	})
}

func TestRoomCount(t *testing.T) {
	ctx := context.Background()

	t.Run("zero when empty", func(t *testing.T) {
		tr, _ := setupTracker(t)
		assert.Equal(t, 0, tr.RoomCount(ctx))
	})

	t.Run("counts active rooms", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)
		_, err = tr.AddCallRoom(ctx, "room2", []string{"c"}, "")
		require.NoError(t, err)

		assert.Equal(t, 2, tr.RoomCount(ctx))
	})

	t.Run("decreases after room collapse", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a"}, "")
		require.NoError(t, err)
		_, err = tr.AddCallRoom(ctx, "room2", []string{"b"}, "")
		require.NoError(t, err)
		assert.Equal(t, 2, tr.RoomCount(ctx))

		_, _, _, err = tr.RemoveUser(ctx, "a")
		require.NoError(t, err)
		assert.Equal(t, 1, tr.RoomCount(ctx))
	})
}

func TestGetUserRoom(t *testing.T) {
	ctx := context.Background()

	t.Run("returns room when user is present", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		roomName, found, err := tr.GetUserRoom(ctx, "a")
		require.NoError(t, err)
		assert.True(t, found)
		assert.Equal(t, "room1", roomName)
	})

	t.Run("returns false when user not in any room", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, found, err := tr.GetUserRoom(ctx, "ghost")
		require.NoError(t, err)
		assert.False(t, found)
	})

	t.Run("reflects removal", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		_, _, _, err = tr.RemoveUser(ctx, "a")
		require.NoError(t, err)

		_, found, err := tr.GetUserRoom(ctx, "a")
		require.NoError(t, err)
		assert.False(t, found)
	})
}

func TestPending(t *testing.T) {
	ctx := context.Background()

	t.Run("set and get round-trips members", func(t *testing.T) {
		tr, _ := setupTracker(t)

		won, err := tr.setPending(ctx, "room1", []string{"a", "b"})
		require.NoError(t, err)
		assert.True(t, won)

		members, err := tr.GetPending(ctx, "room1")
		require.NoError(t, err)
		assert.ElementsMatch(t, []string{"a", "b"}, members)
	})

	t.Run("second set loses SETNX race", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.setPending(ctx, "room1", []string{"a"})
		require.NoError(t, err)

		won, err := tr.setPending(ctx, "room1", []string{"b"})
		require.NoError(t, err)
		assert.False(t, won)

		members, err := tr.GetPending(ctx, "room1")
		require.NoError(t, err)
		assert.ElementsMatch(t, []string{"a"}, members, "first writer's value preserved")
	})

	t.Run("clear allows re-set", func(t *testing.T) {
		tr, _ := setupTracker(t)

		_, err := tr.setPending(ctx, "room1", []string{"a"})
		require.NoError(t, err)

		require.NoError(t, tr.ClearPending(ctx, "room1"))

		won, err := tr.setPending(ctx, "room1", []string{"b"})
		require.NoError(t, err)
		assert.True(t, won)
	})

	t.Run("get returns nil for missing key", func(t *testing.T) {
		tr, _ := setupTracker(t)

		members, err := tr.GetPending(ctx, "nonexistent")
		require.NoError(t, err)
		assert.Nil(t, members)
	})

	t.Run("clear is idempotent", func(t *testing.T) {
		tr, _ := setupTracker(t)
		require.NoError(t, tr.ClearPending(ctx, "nonexistent"))
	})
}

func TestConcurrentAddAndRemove(t *testing.T) {
	ctx := context.Background()

	t.Run("concurrent adds to same room", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a"}, "")
		require.NoError(t, err)

		var wg sync.WaitGroup
		users := []string{"b", "c", "d", "e"}
		errs := make([]error, len(users))
		for i, u := range users {
			wg.Add(1)
			go func(idx int, userID string) {
				defer wg.Done()
				_, errs[idx] = tr.AddUserToRoom(ctx, "room1", userID, "")
			}(i, u)
		}
		wg.Wait()

		for i, err := range errs {
			require.NoError(t, err, "add user %s failed", users[i])
		}

		rooms := readSnapshotRooms(t, mr)
		assert.ElementsMatch(t, []string{"a", "b", "c", "d", "e"}, rooms["room1"].UserIDs)
	})

	t.Run("concurrent removes leave no corrupt state", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b", "c", "d"}, "")
		require.NoError(t, err)

		var wg sync.WaitGroup
		users := []string{"a", "b", "c", "d"}
		for _, u := range users {
			wg.Add(1)
			go func(userID string) {
				defer wg.Done()
				tr.RemoveUser(ctx, userID) //nolint:errcheck
			}(u)
		}
		wg.Wait()

		rooms := readSnapshotRooms(t, mr)
		assert.Empty(t, rooms)
	})

	t.Run("concurrent add and remove: no panic", func(t *testing.T) {
		tr, mr := setupTracker(t)

		_, err := tr.AddCallRoom(ctx, "room1", []string{"a", "b"}, "")
		require.NoError(t, err)

		var wg sync.WaitGroup
		wg.Add(2)
		go func() {
			defer wg.Done()
			tr.AddUserToRoom(ctx, "room1", "c", "") //nolint:errcheck
		}()
		go func() {
			defer wg.Done()
			tr.RemoveUser(ctx, "a") //nolint:errcheck
		}()
		wg.Wait()

		rooms := readSnapshotRooms(t, mr)
		room, ok := rooms["room1"]
		if ok {
			assert.NotContains(t, room.UserIDs, "a", "removed user should be gone")
			assert.Contains(t, room.UserIDs, "b", "untouched user should remain")
		}
	})
}

func TestMergeUserIDs(t *testing.T) {
	t.Run("adds new IDs", func(t *testing.T) {
		result := mergeUserIDs([]string{"a", "b"}, []string{"c"})
		assert.Equal(t, []string{"a", "b", "c"}, result)
	})

	t.Run("deduplicates", func(t *testing.T) {
		result := mergeUserIDs([]string{"a", "b"}, []string{"b", "c"})
		assert.Equal(t, []string{"a", "b", "c"}, result)
	})

	t.Run("handles nil existing", func(t *testing.T) {
		result := mergeUserIDs(nil, []string{"a"})
		assert.Equal(t, []string{"a"}, result)
	})
}

func TestRoomPresenceEqual(t *testing.T) {
	t.Run("equal regardless of order", func(t *testing.T) {
		a := RoomPresence{DisplayName: "Room", UserIDs: []string{"a", "b"}}
		b := RoomPresence{DisplayName: "Room", UserIDs: []string{"b", "a"}}
		assert.True(t, roomPresenceEqual(a, b))
	})

	t.Run("different display names", func(t *testing.T) {
		a := RoomPresence{DisplayName: "X", UserIDs: []string{"a"}}
		b := RoomPresence{DisplayName: "Y", UserIDs: []string{"a"}}
		assert.False(t, roomPresenceEqual(a, b))
	})

	t.Run("different members", func(t *testing.T) {
		a := RoomPresence{UserIDs: []string{"a", "b"}}
		b := RoomPresence{UserIDs: []string{"a", "c"}}
		assert.False(t, roomPresenceEqual(a, b))
	})

	t.Run("different lengths", func(t *testing.T) {
		a := RoomPresence{UserIDs: []string{"a"}}
		b := RoomPresence{UserIDs: []string{"a", "b"}}
		assert.False(t, roomPresenceEqual(a, b))
	})
}
