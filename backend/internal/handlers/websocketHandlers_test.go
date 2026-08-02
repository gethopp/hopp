package handlers

import (
	"context"
	"testing"

	"github.com/alicebob/miniredis/v2"
	"github.com/redis/go-redis/v9"
)

func TestConsumePendingInviteRequiresExactInvite(t *testing.T) {
	mr := miniredis.RunT(t)
	rdb := redis.NewClient(&redis.Options{Addr: mr.Addr()})
	t.Cleanup(func() { rdb.Close() })

	key := pendingInviteKey("inviter", "invitee", "current")
	if err := rdb.Set(context.Background(), key, "1", 0).Err(); err != nil {
		t.Fatal(err)
	}
	if consumePendingInvite(rdb, "inviter", "invitee", "stale") {
		t.Fatal("stale invite consumed the current invite")
	}
	if !consumePendingInvite(rdb, "inviter", "invitee", "current") {
		t.Fatal("matching invite was not consumed")
	}
	if consumePendingInvite(rdb, "inviter", "invitee", "current") {
		t.Fatal("invite was consumed twice")
	}
}
