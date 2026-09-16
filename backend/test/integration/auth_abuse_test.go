//go:build integration
// +build integration

package integration

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/alicebob/miniredis/v2"
	"github.com/labstack/gommon/log"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/config"
	"hopp-backend/internal/models"
	"hopp-backend/internal/server"
)

// setupTestServerWithRedis mirrors setupTestServerFast but wires a miniredis so
// the email-based throttles are active.
func setupTestServerWithRedis(t *testing.T) (*server.Server, func()) {
	mr, err := miniredis.Run()
	require.NoError(t, err)

	cfg := &config.Config{}
	cfg.Server.Port = "8080"
	cfg.Server.Host = "localhost"
	cfg.Server.DeployDomain = "localhost:8080"
	cfg.Database.DSN = "file::memory:?cache=shared"
	cfg.Database.RedisURI = "redis://" + mr.Addr()
	cfg.Auth.SessionSecret = "test-secret-key-for-testing-only"
	cfg.Resend.DefaultSender = "test@example.com"

	srv := server.New(cfg)
	srv.Echo.Logger.SetLevel(log.ERROR)
	require.NoError(t, srv.Initialize())

	cleanup := func() {
		if srv.DB != nil {
			if sqlDB, _ := srv.DB.DB(); sqlDB != nil {
				sqlDB.Close()
			}
		}
		mr.Close()
	}
	return srv, cleanup
}

func postJSON(t *testing.T, srv *server.Server, path string, payload map[string]interface{}) *httptest.ResponseRecorder {
	t.Helper()
	body, err := json.Marshal(payload)
	require.NoError(t, err)
	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	return rec
}

func TestManualSignUp_ShortPasswordRejected(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	rec := postJSON(t, srv, "/api/sign-up", map[string]interface{}{
		"first_name": "Short",
		"last_name":  "Pass",
		"email":      "short.pass@gmail.com",
		"password":   strings.Repeat("a", 11), // 11 chars
	})

	assert.Equal(t, http.StatusBadRequest, rec.Code)

	var count int64
	srv.DB.Model(&models.User{}).Where("email = ?", "short.pass@gmail.com").Count(&count)
	assert.Zero(t, count, "user should not be created with a too-short password")
}

func TestManualSignUp_ExactlyTwelveAccepted(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	rec := postJSON(t, srv, "/api/sign-up", map[string]interface{}{
		"first_name": "Twelve",
		"last_name":  "Chars",
		"email":      "twelve.chars@gmail.com",
		"password":   strings.Repeat("a", 12),
	})

	assert.Equal(t, http.StatusCreated, rec.Code)
}

func TestManualSignUp_OverBcryptCeilingRejected(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	rec := postJSON(t, srv, "/api/sign-up", map[string]interface{}{
		"first_name": "Too",
		"last_name":  "Long",
		"email":      "too.long@gmail.com",
		"password":   strings.Repeat("a", 73),
	})

	assert.Equal(t, http.StatusBadRequest, rec.Code)
}

func TestManualSignUp_DisabledTurnstileIgnoresSuppliedToken(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	// No Turnstile secret is configured in the test config, so a client-supplied
	// token must be ignored rather than trigger verification.
	rec := postJSON(t, srv, "/api/sign-up", map[string]interface{}{
		"first_name":      "Ignored",
		"last_name":       "Token",
		"email":           "ignored.token@gmail.com",
		"password":        "averylongpassword",
		"turnstile_token": "this-should-be-ignored-when-disabled",
	})

	assert.Equal(t, http.StatusCreated, rec.Code)
}

func TestResetPassword_ShortPasswordRejected(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	user := createTestUser(t, srv.DB, "reset.short@gmail.com", "Reset", "Short", "averylongpassword", true)
	resetToken := &models.ResetToken{UserID: user.ID}
	require.NoError(t, resetToken.CreateResetToken(srv.DB))

	rec := patchJSON(t, srv, "/api/reset-password/"+resetToken.Token, map[string]interface{}{
		"password": strings.Repeat("a", 11),
	})

	assert.Equal(t, http.StatusBadRequest, rec.Code)
}

func TestResetPassword_SuccessThenTokenReuseRejected(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	user := createTestUser(t, srv.DB, "reset.reuse@gmail.com", "Reset", "Reuse", "averylongpassword", true)
	resetToken := &models.ResetToken{UserID: user.ID}
	require.NoError(t, resetToken.CreateResetToken(srv.DB))

	// First reset with a valid 12-character password succeeds.
	rec := patchJSON(t, srv, "/api/reset-password/"+resetToken.Token, map[string]interface{}{
		"password": "brandnewpass12",
	})
	assert.Equal(t, http.StatusOK, rec.Code)

	// Reusing the same (now consumed) token is rejected.
	rec = patchJSON(t, srv, "/api/reset-password/"+resetToken.Token, map[string]interface{}{
		"password": "anotherpass123",
	})
	assert.Equal(t, http.StatusBadRequest, rec.Code)
}

func TestSignIn_RateLimitedAfterFiveAttempts(t *testing.T) {
	srv, cleanup := setupTestServerWithRedis(t)
	defer cleanup()

	createTestUser(t, srv.DB, "throttle@gmail.com", "Throttle", "Me", "correcthorsebattery", true)

	// Five wrong-password attempts are allowed through to the credential check (401).
	for i := 0; i < 5; i++ {
		rec := postJSON(t, srv, "/api/sign-in", map[string]interface{}{
			"email":    "throttle@gmail.com",
			"password": "wrong-password",
		})
		require.Equalf(t, http.StatusUnauthorized, rec.Code, "attempt %d expected 401", i+1)
	}

	// The sixth is throttled before the credential check.
	rec := postJSON(t, srv, "/api/sign-in", map[string]interface{}{
		"email":    "throttle@gmail.com",
		"password": "wrong-password",
	})
	assert.Equal(t, http.StatusTooManyRequests, rec.Code)
	assert.NotEmpty(t, rec.Header().Get("Retry-After"))
}

func TestForgotPassword_ThrottleSendsOneEmailPerWindow(t *testing.T) {
	srv, cleanup := setupTestServerWithRedis(t)
	defer cleanup()

	user := createTestUser(t, srv.DB, "forgot.throttle@gmail.com", "Forgot", "Throttle", "averylongpassword", true)

	// Two requests within the window both return the generic 200 response.
	for i := 0; i < 2; i++ {
		rec := postJSON(t, srv, "/api/forgot-password", map[string]interface{}{
			"email": "forgot.throttle@gmail.com",
		})
		require.Equal(t, http.StatusOK, rec.Code)
	}

	// But the throttle short-circuits the second request before token creation,
	// so only one reset token exists.
	var count int64
	srv.DB.Model(&models.ResetToken{}).Where("user_id = ?", user.ID).Count(&count)
	assert.Equal(t, int64(1), count, "throttle should limit to one reset token per window")
}

func TestForgotPassword_AntiEnumerationWhileThrottled(t *testing.T) {
	srv, cleanup := setupTestServerWithRedis(t)
	defer cleanup()

	// A nonexistent address returns the same generic 200 both before and after
	// the throttle would kick in — behavior doesn't diverge by account existence.
	for i := 0; i < 3; i++ {
		rec := postJSON(t, srv, "/api/forgot-password", map[string]interface{}{
			"email": "does-not-exist@gmail.com",
		})
		require.Equalf(t, http.StatusOK, rec.Code, "request %d should be a generic 200", i+1)

		var body map[string]string
		require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &body))
		assert.Contains(t, body["message"], "If the email you specified exists")
	}
}

// patchJSON is a small helper for PATCH requests with a JSON body.
func patchJSON(t *testing.T, srv *server.Server, path string, payload map[string]interface{}) *httptest.ResponseRecorder {
	t.Helper()
	body, err := json.Marshal(payload)
	require.NoError(t, err)
	req := httptest.NewRequest(http.MethodPatch, path, bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	return rec
}
