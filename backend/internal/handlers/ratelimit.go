package handlers

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/labstack/echo/v4"
	"github.com/redis/go-redis/v9"
)

// These throttles use a fixed 60-second-window counter; the simplest
// useful control. A request in the window increments a per-email counter that
// expires after the window elapses.
const (
	rateLimitWindow = 60 * time.Second

	// signInMaxAttempts allows five sign-in attempts per normalized email per
	// window; the sixth is rejected. This causes at most a short attacker-induced
	// delay, never a permanent account lockout.
	signInMaxAttempts = 5

	// resetEmailMaxPerWindow sends at most one password-reset email per
	// normalized address per window.
	resetEmailMaxPerWindow = 1
)

// normalizedEmailRateKey hashes strings.ToLower(strings.TrimSpace(email)) so raw
// email addresses are never stored in Redis keys.
func normalizedEmailRateKey(scope, email string) string {
	normalized := strings.ToLower(strings.TrimSpace(email))
	sum := sha256.Sum256([]byte(normalized))
	return fmt.Sprintf("ratelimit:%s:%s", scope, hex.EncodeToString(sum[:]))
}

// incrementRateLimit increments the counter at key and returns the new count and
// the TTL remaining on the window. The expiry is set only on the first increment
// so the window stays anchored to the first request instead of sliding.
func incrementRateLimit(ctx context.Context, rdb *redis.Client, key string) (int64, time.Duration, error) {
	count, err := rdb.Incr(ctx, key).Result()
	if err != nil {
		return 0, 0, err
	}
	if count == 1 {
		if err := rdb.Expire(ctx, key, rateLimitWindow).Err(); err != nil {
			return count, 0, err
		}
		return count, rateLimitWindow, nil
	}
	ttl, err := rdb.TTL(ctx, key).Result()
	if err != nil {
		return count, 0, err
	}
	return count, ttl, nil
}

// checkSignInRateLimit enforces the per-email sign-in throttle. It fails open
// (returns nil) when Redis is unavailable. The limiter short-circuits on a nil
// client, not just on command errors because Turnstile remains active. On the
// sixth attempt within the window it returns a 429 with a Retry-After header.
func (h *AuthHandler) checkSignInRateLimit(c echo.Context, email string) error {
	if h.Redis == nil {
		return nil
	}

	key := normalizedEmailRateKey("signin", email)
	count, ttl, err := incrementRateLimit(c.Request().Context(), h.Redis, key)
	if err != nil {
		c.Logger().Errorf("sign-in rate limit: redis error, failing open: %v", err)
		return nil
	}

	if count > signInMaxAttempts {
		retryAfter := int(ttl.Seconds())
		if retryAfter < 1 {
			retryAfter = 1
		}
		c.Response().Header().Set("Retry-After", strconv.Itoa(retryAfter))
		return echo.NewHTTPError(http.StatusTooManyRequests, "Too many sign-in attempts. Please try again later.")
	}

	return nil
}

// clearSignInRateLimit resets the sign-in counter after a successful login
func (h *AuthHandler) clearSignInRateLimit(c echo.Context, email string) {
	if h.Redis == nil {
		return
	}
	key := normalizedEmailRateKey("signin", email)
	if err := h.Redis.Del(c.Request().Context(), key).Err(); err != nil {
		c.Logger().Warnf("sign-in rate limit: failed to clear counter: %v", err)
	}
}

// allowResetEmail reports whether a password-reset email may be sent to email
// during the current window. It increments the counter unconditionally (callers
// must invoke it uniformly, before any user lookup, so behavior and timing don't
// diverge by account existence) and fails open when Redis is unavailable.
func (h *AuthHandler) allowResetEmail(c echo.Context, email string) bool {
	if h.Redis == nil {
		return true
	}

	key := normalizedEmailRateKey("reset", email)
	count, _, err := incrementRateLimit(c.Request().Context(), h.Redis, key)
	if err != nil {
		c.Logger().Errorf("reset-email rate limit: redis error, failing open: %v", err)
		return true
	}

	return count <= resetEmailMaxPerWindow
}
