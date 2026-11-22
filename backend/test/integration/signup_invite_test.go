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

	"github.com/markbates/goth"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/handlers"
	"hopp-backend/internal/models"
	"hopp-backend/internal/server"
)

// createInstanceOwner creates the earliest account, as an admin of a team that
// actually exists.
func createInstanceOwner(t *testing.T, srv *server.Server, email string) *models.User {
	t.Helper()
	team := createTestTeam(t, srv.DB, "Owner Team")
	owner := createTestUser(t, srv.DB, email, "Owner", "User", "averylongpassword", true)
	require.NoError(t, srv.DB.Model(&models.User{}).Where("id = ?", owner.ID).Update("team_id", team.ID).Error)
	owner.TeamID = &team.ID
	return owner
}

func teamCount(t *testing.T, srv *server.Server) int64 {
	t.Helper()
	var count int64
	require.NoError(t, srv.DB.Model(&models.Team{}).Count(&count).Error)
	return count
}

// requireInvite turns on the instance setting the way the web UI does.
func requireInvite(t *testing.T, srv *server.Server) {
	t.Helper()
	_, err := models.SetSignupRequiresInvite(srv.DB, true)
	require.NoError(t, err)
}

func signUpPayload(email, inviteUUID string) map[string]interface{} {
	p := map[string]interface{}{
		"first_name": "Invite",
		"last_name":  "Test",
		"email":      email,
		"password":   "averylongpassword",
	}
	if inviteUUID != "" {
		p["team_invite_uuid"] = inviteUUID
	}
	return p
}

func userCount(t *testing.T, srv *server.Server, email string) int64 {
	t.Helper()
	var count int64
	require.NoError(t, srv.DB.Model(&models.User{}).Where("email = ?", email).Count(&count).Error)
	return count
}

func TestSignUpRequiresInvite_FirstAccountBootstraps(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	requireInvite(t, srv)

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("first.admin@gmail.com", ""))

	assert.Equal(t, http.StatusCreated, rec.Code, "an empty instance must accept its first account without an invite")
	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "first.admin@gmail.com").First(&user).Error)
	assert.True(t, user.IsAdmin, "the bootstrap account becomes the team admin")
}

func TestSignUpRequiresInvite_RejectsWithoutInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	requireInvite(t, srv)
	createInstanceOwner(t, srv, "owner@gmail.com")

	teamsBefore := teamCount(t, srv)

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("stranger@gmail.com", ""))

	assert.Equal(t, http.StatusForbidden, rec.Code)
	assert.Contains(t, rec.Body.String(), "invitation only")
	assert.Zero(t, userCount(t, srv, "stranger@gmail.com"), "no account may be created without an invite")
	assert.Equal(t, teamsBefore, teamCount(t, srv), "a rejected sign-up must not leave a team behind")
}

func TestSignUpRequiresInvite_RejectsUnknownInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	requireInvite(t, srv)
	createInstanceOwner(t, srv, "owner2@gmail.com")

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("stranger2@gmail.com", "00000000-0000-0000-0000-000000000000"))

	assert.Equal(t, http.StatusForbidden, rec.Code, "an unknown invite must not fall through to creating a new team")
	assert.Zero(t, userCount(t, srv, "stranger2@gmail.com"))
}

func TestSignUpRequiresInvite_AcceptsValidInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	requireInvite(t, srv)
	owner := createInstanceOwner(t, srv, "owner3@gmail.com")
	invitation := models.TeamInvitation{TeamID: int(*owner.TeamID), UniqueID: "11111111-1111-1111-1111-111111111111"}
	require.NoError(t, srv.DB.Create(&invitation).Error)

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("invited@gmail.com", invitation.UniqueID))

	assert.Equal(t, http.StatusCreated, rec.Code)
	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "invited@gmail.com").First(&user).Error)
	require.NotNil(t, user.TeamID)
	assert.Equal(t, *owner.TeamID, *user.TeamID, "invited user joins the inviting team")
	assert.False(t, user.IsAdmin)
}

func TestSignUpOpenByDefault(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner4@gmail.com")

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("open.signup@gmail.com", ""))

	assert.Equal(t, http.StatusCreated, rec.Code, "with the setting left at its default sign-up stays open (upstream behavior)")
}

func authedJSON(t *testing.T, srv *server.Server, method, path, email string, payload map[string]interface{}) *httptest.ResponseRecorder {
	t.Helper()
	var body []byte
	if payload != nil {
		var err error
		body, err = json.Marshal(payload)
		require.NoError(t, err)
	}
	req := httptest.NewRequest(method, path, bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+getJWTToken(t, srv, email))
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	return rec
}

func TestInstanceSettings_DefaultsToOpenSignUp(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner5@gmail.com")

	rec := authedJSON(t, srv, http.MethodGet, "/api/auth/instance-settings", "owner5@gmail.com", nil)

	require.Equal(t, http.StatusOK, rec.Code)
	var resp map[string]interface{}
	require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &resp))
	assert.Equal(t, false, resp["signup_requires_invite"], "default must match pre-existing behaviour")
	assert.Equal(t, true, resp["is_owner"])
}

func TestInstanceSettings_OwnerCanToggle(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner6@gmail.com")

	rec := authedJSON(t, srv, http.MethodPut, "/api/auth/instance-settings", "owner6@gmail.com",
		map[string]interface{}{"signup_requires_invite": true})
	require.Equal(t, http.StatusOK, rec.Code)
	assert.JSONEq(t, `{"is_owner":true,"signup_requires_invite":true}`, rec.Body.String())

	// The toggle takes effect immediately for sign-up.
	blocked := postJSON(t, srv, "/api/sign-up", signUpPayload("late.stranger@gmail.com", ""))
	assert.Equal(t, http.StatusForbidden, blocked.Code)

	rec = authedJSON(t, srv, http.MethodPut, "/api/auth/instance-settings", "owner6@gmail.com",
		map[string]interface{}{"signup_requires_invite": false})
	require.Equal(t, http.StatusOK, rec.Code)
	reopened := postJSON(t, srv, "/api/sign-up", signUpPayload("late.stranger@gmail.com", ""))
	assert.Equal(t, http.StatusCreated, reopened.Code)
}

func TestInstanceSettings_NonOwnerCannotToggle(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner7@gmail.com")
	// A later account is a team admin of its own team but not the instance owner.
	other := createTestUser(t, srv.DB, "other7@gmail.com", "Other", "Seven", "averylongpassword", false)
	require.NoError(t, srv.DB.Model(&models.User{}).Where("id = ?", other.ID).Update("is_admin", true).Error)
	requireInvite(t, srv)

	rec := authedJSON(t, srv, http.MethodPut, "/api/auth/instance-settings", "other7@gmail.com",
		map[string]interface{}{"signup_requires_invite": false})

	assert.Equal(t, http.StatusForbidden, rec.Code)
	settings, err := models.GetInstanceSettings(srv.DB)
	require.NoError(t, err)
	assert.True(t, settings.SignupRequiresInvite, "a non-owner team admin must not reopen sign-up")

	get := authedJSON(t, srv, http.MethodGet, "/api/auth/instance-settings", "other7@gmail.com", nil)
	require.Equal(t, http.StatusOK, get.Code)
	assert.Contains(t, get.Body.String(), `"is_owner":false`)
}

func TestInstanceSettings_RequiresAuth(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	req := httptest.NewRequest(http.MethodPut, "/api/auth/instance-settings", strings.NewReader(`{"signup_requires_invite":false}`))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)

	assert.Equal(t, http.StatusUnauthorized, rec.Code)
}

func TestInstanceSettings_RejectsMissingField(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner8@gmail.com")

	rec := authedJSON(t, srv, http.MethodPut, "/api/auth/instance-settings", "owner8@gmail.com", map[string]interface{}{})

	assert.Equal(t, http.StatusBadRequest, rec.Code, "an empty body must not silently turn the setting off")
}

func TestInstanceSettings_RejectsWrongType(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner9@gmail.com")
	requireInvite(t, srv)

	rec := authedJSON(t, srv, http.MethodPut, "/api/auth/instance-settings", "owner9@gmail.com",
		map[string]interface{}{"signup_requires_invite": "false"})

	assert.Equal(t, http.StatusBadRequest, rec.Code)
	settings, err := models.GetInstanceSettings(srv.DB)
	require.NoError(t, err)
	assert.True(t, settings.SignupRequiresInvite)
}

// socialCallback drives SocialLoginCallback as if the provider had just
// authenticated the given user, optionally with an invite stored in the session
// by SocialLogin.
func socialCallback(t *testing.T, srv *server.Server, email, inviteUUID string) *httptest.ResponseRecorder {
	t.Helper()
	provider := &MockSocialAuthProvider{User: goth.User{Email: email, FirstName: "Social", LastName: "User"}}
	authHandler := handlers.NewAuthHandler(srv.DB, srv.Config, srv.JwtIssuer, srv.Redis, provider)
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

func TestSignUpRequiresInvite_SocialRejectsWithoutInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner10@gmail.com")
	requireInvite(t, srv)
	teamsBefore := teamCount(t, srv)

	rec := socialCallback(t, srv, "social.stranger@gmail.com", "")

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Equal(t, "/login?error=invite_required", rec.Header().Get("Location"))
	assert.Zero(t, userCount(t, srv, "social.stranger@gmail.com"), "no account may be created without an invite")
	assert.Equal(t, teamsBefore, teamCount(t, srv), "the rolled-back sign-up must not leave a team behind")
}

func TestSignUpRequiresInvite_SocialRejectsUnknownInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner11@gmail.com")
	requireInvite(t, srv)

	rec := socialCallback(t, srv, "social.stale@gmail.com", "00000000-0000-0000-0000-000000000000")

	assert.Equal(t, "/login?error=invite_required", rec.Header().Get("Location"))
	assert.Zero(t, userCount(t, srv, "social.stale@gmail.com"))
}

func TestSignUpRequiresInvite_SocialAcceptsValidInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	owner := createInstanceOwner(t, srv, "owner12@gmail.com")
	requireInvite(t, srv)
	invitation := models.TeamInvitation{TeamID: int(*owner.TeamID), UniqueID: "22222222-2222-2222-2222-222222222222"}
	require.NoError(t, srv.DB.Create(&invitation).Error)

	rec := socialCallback(t, srv, "social.invited@gmail.com", invitation.UniqueID)

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")
	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "social.invited@gmail.com").First(&user).Error)
	assert.Equal(t, *owner.TeamID, *user.TeamID)
}

func TestSignUpRequiresInvite_SocialExistingUserCanStillSignIn(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	createInstanceOwner(t, srv, "owner13@gmail.com")
	requireInvite(t, srv)

	rec := socialCallback(t, srv, "owner13@gmail.com", "")

	assert.Contains(t, rec.Header().Get("Location"), "/login?token=", "the setting must not lock out existing accounts")
}
