//go:build integration
// +build integration

package integration

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/google/uuid"
	"github.com/labstack/gommon/log"
	"github.com/markbates/goth"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/config"
	"hopp-backend/internal/handlers"
	"hopp-backend/internal/models"
	"hopp-backend/internal/server"
)

// setupTestServerWithConfig builds a test server like setupTestServerFast, but
// lets the caller adjust the config before the server is initialized.
func setupTestServerWithConfig(t *testing.T, configure func(cfg *config.Config)) (*server.Server, func()) {
	cfg := &config.Config{}
	cfg.Server.Port = "8080"
	cfg.Server.Host = "localhost"
	cfg.Server.DeployDomain = "localhost:8080"
	cfg.Server.Debug = false
	cfg.Database.DSN = "file::memory:?cache=shared"
	cfg.Database.RedisURI = ""
	cfg.Auth.SessionSecret = "test-secret-key-for-testing-only"
	cfg.Resend.DefaultSender = "test@example.com"
	configure(cfg)

	srv := server.New(cfg)
	srv.Echo.Logger.SetLevel(log.ERROR)

	require.NoError(t, srv.Initialize())

	cleanup := func() {
		if srv.DB != nil {
			if sqlDB, _ := srv.DB.DB(); sqlDB != nil {
				sqlDB.Close()
			}
		}
	}

	return srv, cleanup
}

func setupTestServerSignupDisabled(t *testing.T) (*server.Server, func()) {
	return setupTestServerWithConfig(t, func(cfg *config.Config) {
		cfg.Auth.DisableSignup = true
	})
}

func signUpPayload(email string) map[string]interface{} {
	return map[string]interface{}{
		"first_name": "Jane",
		"last_name":  "Doe",
		"email":      email,
		"password":   "securepassword123",
	}
}

func createTestInvitation(t *testing.T, srv *server.Server, teamID uint) string {
	inviteUUID, err := uuid.NewV7()
	require.NoError(t, err)
	invitation := models.TeamInvitation{TeamID: int(teamID), UniqueID: inviteUUID.String()}
	require.NoError(t, srv.DB.Create(&invitation).Error)
	return inviteUUID.String()
}

func userCount(t *testing.T, srv *server.Server) int64 {
	var count int64
	require.NoError(t, srv.DB.Model(&models.User{}).Count(&count).Error)
	return count
}

// socialCallback runs the social login callback with a mocked provider result
// and an optional team invitation stored in the session.
func socialCallback(t *testing.T, srv *server.Server, user goth.User, inviteUUID string) *httptest.ResponseRecorder {
	authHandler := handlers.NewAuthHandler(srv.DB, srv.Config, srv.JwtIssuer, srv.Redis, &MockSocialAuthProvider{User: user})
	authHandler.ServerState.EmailClient = srv.EmailClient
	srv.Echo.Router().Add(http.MethodGet, "/api/auth/social/:provider/callback", authHandler.SocialLoginCallback)

	req := httptest.NewRequest(http.MethodGet, "/api/auth/social/google/callback", nil)
	rec := httptest.NewRecorder()

	if inviteUUID != "" {
		sess, _ := srv.Store.Get(req, "session")
		sess.Values["team_invite_uuid"] = inviteUUID
		require.NoError(t, sess.Save(req, rec))
		req.Header.Set("Cookie", rec.Header().Get("Set-Cookie"))
	}

	srv.Echo.ServeHTTP(rec, req)
	return rec
}

func TestInstanceConfig_Defaults(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	req := httptest.NewRequest(http.MethodGet, "/api/config", nil)
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)

	require.Equal(t, http.StatusOK, rec.Code)
	var resp handlers.InstanceConfigResponse
	require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &resp))
	assert.True(t, resp.SignupEnabled)
	assert.True(t, resp.PasswordLoginEnabled)
	assert.False(t, resp.BillingEnabled)
	assert.Empty(t, resp.AuthProviders)
	// Clients iterate over the list, so it must serialize as [] and not null.
	assert.Contains(t, rec.Body.String(), `"auth_providers":[]`)
}

func TestInstanceConfig_ReflectsFlags(t *testing.T) {
	srv, cleanup := setupTestServerWithConfig(t, func(cfg *config.Config) {
		cfg.Auth.DisableSignup = true
		cfg.Auth.DisablePasswordLogin = true
		cfg.Auth.GitHubKey = "key"
		cfg.Auth.GitHubSecret = "secret"
		cfg.Auth.GoogleKey = "key-without-secret"
		cfg.Stripe.SecretKey = "sk_test_dummy"
	})
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	req := httptest.NewRequest(http.MethodGet, "/api/config", nil)
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)

	require.Equal(t, http.StatusOK, rec.Code)
	var resp handlers.InstanceConfigResponse
	require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &resp))
	assert.False(t, resp.SignupEnabled)
	assert.False(t, resp.PasswordLoginEnabled)
	assert.True(t, resp.BillingEnabled)
	assert.Equal(t, []string{"github"}, resp.AuthProviders)
}

func TestInstanceConfig_SignupEnabledWhileInstanceIsEmpty(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()

	req := httptest.NewRequest(http.MethodGet, "/api/config", nil)
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)

	require.Equal(t, http.StatusOK, rec.Code)
	var resp handlers.InstanceConfigResponse
	require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &resp))
	assert.True(t, resp.SignupEnabled)
}

func TestSocialProviders_OnlyConfiguredAreRegistered(t *testing.T) {
	_, cleanup := setupTestServerWithConfig(t, func(cfg *config.Config) {
		cfg.Auth.GitHubKey = "key"
		cfg.Auth.GitHubSecret = "secret"
	})
	defer cleanup()

	providers := goth.GetProviders()
	assert.Len(t, providers, 1)
	assert.Contains(t, providers, "github")
}

func TestDisableSignup_FirstAccountAllowed(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("first@gmail.com"))
	require.Equal(t, http.StatusCreated, rec.Code, rec.Body.String())

	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "first@gmail.com").First(&user).Error)
	assert.True(t, user.IsAdmin)
	assert.NotNil(t, user.TeamID)
}

func TestDisableSignup_UninvitedRejected(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("stranger@gmail.com"))

	assert.Equal(t, http.StatusForbidden, rec.Code)
	assert.Equal(t, int64(1), userCount(t, srv))

	// The rejected sign-up must not leave an orphan team behind.
	var teams int64
	require.NoError(t, srv.DB.Model(&models.Team{}).Count(&teams).Error)
	assert.Equal(t, int64(1), teams)
}

func TestDisableSignup_InvalidInviteRejected(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	payload := signUpPayload("stranger@gmail.com")
	payload["team_invite_uuid"] = "not-a-real-invitation"
	rec := postJSON(t, srv, "/api/sign-up", payload)

	assert.Equal(t, http.StatusForbidden, rec.Code)
	assert.Equal(t, int64(1), userCount(t, srv))
}

func TestDisableSignup_InvitedAllowed(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	admin := createTestUser(t, srv.DB, "admin@gmail.com", "Admin", "User", "securepassword123", false)

	payload := signUpPayload("invited@gmail.com")
	payload["team_invite_uuid"] = createTestInvitation(t, srv, *admin.TeamID)
	rec := postJSON(t, srv, "/api/sign-up", payload)

	require.Equal(t, http.StatusCreated, rec.Code, rec.Body.String())
	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "invited@gmail.com").First(&user).Error)
	assert.Equal(t, *admin.TeamID, *user.TeamID)
	assert.False(t, user.IsAdmin)
}

func TestDisableSignup_SocialUninvitedRejected(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	rec := socialCallback(t, srv, goth.User{Email: "stranger@gmail.com", FirstName: "Stranger"}, "")

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Equal(t, "/login?error=signup_disabled", rec.Header().Get("Location"))
	assert.Equal(t, int64(1), userCount(t, srv))
}

func TestDisableSignup_SocialInvitedAllowed(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	admin := createTestUser(t, srv.DB, "admin@gmail.com", "Admin", "User", "securepassword123", false)
	invite := createTestInvitation(t, srv, *admin.TeamID)

	rec := socialCallback(t, srv, goth.User{Email: "invited@gmail.com", FirstName: "Invited"}, invite)

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")
	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "invited@gmail.com").First(&user).Error)
	assert.Equal(t, *admin.TeamID, *user.TeamID)
}

func TestDisableSignup_SocialExistingUserCanSignIn(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	rec := socialCallback(t, srv, goth.User{Email: "existing@gmail.com", FirstName: "Existing"}, "")

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")
}

func TestDisableSignup_ExistingUserCanSignIn(t *testing.T) {
	srv, cleanup := setupTestServerSignupDisabled(t)
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	rec := postJSON(t, srv, "/api/sign-in", map[string]interface{}{
		"email":    "existing@gmail.com",
		"password": "securepassword123",
	})

	assert.Equal(t, http.StatusOK, rec.Code, rec.Body.String())
}

func TestDisablePasswordLogin_EndpointsRejected(t *testing.T) {
	srv, cleanup := setupTestServerWithConfig(t, func(cfg *config.Config) {
		cfg.Auth.DisablePasswordLogin = true
		cfg.Auth.GitHubKey = "key"
		cfg.Auth.GitHubSecret = "secret"
	})
	defer cleanup()
	createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "securepassword123", false)

	cases := []struct {
		name    string
		method  string
		path    string
		payload map[string]interface{}
	}{
		{"sign-in", http.MethodPost, "/api/sign-in", map[string]interface{}{"email": "existing@gmail.com", "password": "securepassword123"}},
		{"sign-up", http.MethodPost, "/api/sign-up", signUpPayload("new@gmail.com")},
		{"forgot-password", http.MethodPost, "/api/forgot-password", map[string]interface{}{"email": "existing@gmail.com"}},
		{"reset-password", http.MethodPatch, "/api/reset-password/some-token", map[string]interface{}{"password": "anothersecurepassword"}},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			body, err := json.Marshal(tc.payload)
			require.NoError(t, err)
			req := httptest.NewRequest(tc.method, tc.path, bytes.NewReader(body))
			req.Header.Set("Content-Type", "application/json")
			rec := httptest.NewRecorder()
			srv.Echo.ServeHTTP(rec, req)

			assert.Equal(t, http.StatusForbidden, rec.Code, rec.Body.String())
		})
	}

	assert.Equal(t, int64(1), userCount(t, srv))
}

func TestDisablePasswordLogin_SocialLoginStillWorks(t *testing.T) {
	srv, cleanup := setupTestServerWithConfig(t, func(cfg *config.Config) {
		cfg.Auth.DisablePasswordLogin = true
		cfg.Auth.GitHubKey = "key"
		cfg.Auth.GitHubSecret = "secret"
	})
	defer cleanup()

	rec := socialCallback(t, srv, goth.User{Email: "new@gmail.com", FirstName: "New"}, "")

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")
}
