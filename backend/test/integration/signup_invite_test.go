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

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/models"
	"hopp-backend/internal/server"
)

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
	createTestUser(t, srv.DB, "owner@gmail.com", "Owner", "One", "averylongpassword", true)

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("stranger@gmail.com", ""))

	assert.Equal(t, http.StatusForbidden, rec.Code)
	assert.Zero(t, userCount(t, srv, "stranger@gmail.com"), "no account may be created without an invite")
	var teams int64
	require.NoError(t, srv.DB.Model(&models.Team{}).Count(&teams).Error)
	assert.Zero(t, teams, "a rejected sign-up must not leave a team behind")
}

func TestSignUpRequiresInvite_RejectsUnknownInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	requireInvite(t, srv)
	createTestUser(t, srv.DB, "owner2@gmail.com", "Owner", "Two", "averylongpassword", true)

	rec := postJSON(t, srv, "/api/sign-up", signUpPayload("stranger2@gmail.com", "00000000-0000-0000-0000-000000000000"))

	assert.Equal(t, http.StatusForbidden, rec.Code, "an unknown invite must not fall through to creating a new team")
	assert.Zero(t, userCount(t, srv, "stranger2@gmail.com"))
}

func TestSignUpRequiresInvite_AcceptsValidInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()
	requireInvite(t, srv)
	owner := createTestUser(t, srv.DB, "owner3@gmail.com", "Owner", "Three", "averylongpassword", true)
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
	createTestUser(t, srv.DB, "owner4@gmail.com", "Owner", "Four", "averylongpassword", true)

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
	createTestUser(t, srv.DB, "owner5@gmail.com", "Owner", "Five", "averylongpassword", true)

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
	createTestUser(t, srv.DB, "owner6@gmail.com", "Owner", "Six", "averylongpassword", true)

	rec := authedJSON(t, srv, http.MethodPut, "/api/auth/instance-settings", "owner6@gmail.com",
		map[string]interface{}{"signup_requires_invite": true})
	require.Equal(t, http.StatusOK, rec.Code)

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
	createTestUser(t, srv.DB, "owner7@gmail.com", "Owner", "Seven", "averylongpassword", true)
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
