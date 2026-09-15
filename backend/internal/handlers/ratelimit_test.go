package handlers

import (
	"net/http"
	"testing"
	"time"

	"hopp-backend/internal/common"

	"github.com/alicebob/miniredis/v2"
	"github.com/redis/go-redis/v9"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func newRedisHandler(t *testing.T) (*AuthHandler, *miniredis.Miniredis) {
	t.Helper()
	mr, err := miniredis.Run()
	require.NoError(t, err)
	t.Cleanup(mr.Close)

	client := redis.NewClient(&redis.Options{Addr: mr.Addr()})
	t.Cleanup(func() { _ = client.Close() })

	return &AuthHandler{ServerState: common.ServerState{Redis: client}}, mr
}

func TestCheckSignInRateLimit_AllowsFiveThenBlocks(t *testing.T) {
	h, _ := newRedisHandler(t)

	for i := 0; i < signInMaxAttempts; i++ {
		c, _ := newTestContext()
		require.NoErrorf(t, h.checkSignInRateLimit(c, "User@Example.com"), "attempt %d should be allowed", i+1)
	}

	// The sixth attempt is blocked. Normalization means casing/whitespace map to the same key.
	c, rec := newTestContext()
	err := h.checkSignInRateLimit(c, "  user@example.com ")
	assert.Equal(t, http.StatusTooManyRequests, httpErrorCode(t, err))
	assert.NotEmpty(t, rec.Header().Get("Retry-After"))
}

func TestCheckSignInRateLimit_ClearResetsCounter(t *testing.T) {
	h, _ := newRedisHandler(t)

	for i := 0; i < signInMaxAttempts; i++ {
		c, _ := newTestContext()
		require.NoError(t, h.checkSignInRateLimit(c, "a@b.com"))
	}

	clearCtx, _ := newTestContext()
	h.clearSignInRateLimit(clearCtx, "a@b.com")

	// After clearing, a fresh window begins.
	c, _ := newTestContext()
	require.NoError(t, h.checkSignInRateLimit(c, "a@b.com"))
}

func TestCheckSignInRateLimit_Expiry(t *testing.T) {
	h, mr := newRedisHandler(t)

	for i := 0; i < signInMaxAttempts; i++ {
		c, _ := newTestContext()
		require.NoError(t, h.checkSignInRateLimit(c, "a@b.com"))
	}

	blocked, _ := newTestContext()
	require.Error(t, h.checkSignInRateLimit(blocked, "a@b.com"))

	// After the window elapses the counter expires and attempts are allowed again.
	mr.FastForward(rateLimitWindow + time.Second)
	c, _ := newTestContext()
	require.NoError(t, h.checkSignInRateLimit(c, "a@b.com"))
}

func TestAllowResetEmail_OnePerWindow(t *testing.T) {
	h, mr := newRedisHandler(t)

	c1, _ := newTestContext()
	assert.True(t, h.allowResetEmail(c1, "a@b.com"))

	c2, _ := newTestContext()
	assert.False(t, h.allowResetEmail(c2, "A@B.com")) // normalized to same key

	mr.FastForward(rateLimitWindow + time.Second)
	c3, _ := newTestContext()
	assert.True(t, h.allowResetEmail(c3, "a@b.com"))
}

func TestRateLimit_NilRedisShortCircuits(t *testing.T) {
	h := &AuthHandler{ServerState: common.ServerState{Redis: nil}}

	// With no Redis client the limiter fails open: never blocks, always allows.
	for i := 0; i < signInMaxAttempts*3; i++ {
		c, _ := newTestContext()
		require.NoError(t, h.checkSignInRateLimit(c, "a@b.com"))
	}

	c, _ := newTestContext()
	assert.True(t, h.allowResetEmail(c, "a@b.com"))

	// clear must be a no-op and not panic.
	clearCtx, _ := newTestContext()
	h.clearSignInRateLimit(clearCtx, "a@b.com")
}
