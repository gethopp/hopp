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

	"gorm.io/gorm"
)

// setupTestServerFast creates a test server with SQLite in-memory and optional Redis
// This is much faster than using containers (no Docker needed, no container startup time)
// It uses the actual server.Initialize() method to avoid code duplication
func setupTestServerFast(t *testing.T) (*server.Server, func()) {
	// Create test config with SQLite DSN (server will auto-detect SQLite driver)
	cfg := &config.Config{}
	cfg.Server.Port = "8080"
	cfg.Server.Host = "localhost"
	cfg.Server.DeployDomain = "localhost:8080"
	cfg.Server.Debug = false
	cfg.Database.DSN = "file::memory:?cache=shared" // SQLite in-memory - server will detect and use SQLite driver
	cfg.Database.RedisURI = ""                      // Empty Redis URI - server will skip Redis setup
	cfg.Auth.SessionSecret = "test-secret-key-for-testing-only"
	cfg.Resend.DefaultSender = "test@example.com"

	// Create server using the actual server.New() method
	srv := server.New(cfg)
	srv.Echo.Logger.SetLevel(log.ERROR)

	// Initialize server - this will use SQLite (detected from DSN) and skip Redis (empty URI)
	err := srv.Initialize()
	require.NoError(t, err)

	// Cleanup function (SQLite in-memory is automatically cleaned up)
	cleanup := func() {
		// SQLite in-memory database is automatically cleaned up when connection closes
		// But we can explicitly close if needed
		if srv.DB != nil {
			sqlDB, _ := srv.DB.DB()
			if sqlDB != nil {
				sqlDB.Close()
			}
		}
	}

	return srv, cleanup
}

// createTestUser is a helper to create a user in the test database
func createTestUser(t *testing.T, db *gorm.DB, email, firstName, lastName, password string, isAdmin bool) *models.User {
	teamID := uint(1)
	if !isAdmin {
		// For non-admin, we'll create a team first
		team := models.Team{Name: "Test Team"}
		err := db.Create(&team).Error
		require.NoError(t, err)
		teamID = team.ID
	}

	user := &models.User{
		FirstName: firstName,
		LastName:  lastName,
		Email:     email,
		Password:  password,
		TeamID:    &teamID,
		IsAdmin:   isAdmin,
		EmailSubscriptions: models.EmailSubscriptions{
			MarketingEmails: true,
		},
	}

	err := db.Create(user).Error
	require.NoError(t, err)

	return user
}

func TestManualSignUp_NewUser(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	// Test request
	signUpReq := map[string]interface{}{
		"first_name": "John",
		"last_name":  "Doe",
		"email":      "john.doe@gmail.com",
		"password":   "securepassword123",
		"team_name":  "John's Team",
	}

	body, err := json.Marshal(signUpReq)
	require.NoError(t, err)

	req := httptest.NewRequest(http.MethodPost, "/api/sign-up", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()

	// Execute
	srv.Echo.ServeHTTP(rec, req)

	// Assertions
	if rec.Code != http.StatusCreated {
		t.Logf("Response body: %s", rec.Body.String())
	}
	assert.Equal(t, http.StatusCreated, rec.Code)

	var response map[string]string
	err = json.Unmarshal(rec.Body.Bytes(), &response)
	require.NoError(t, err)
	assert.NotEmpty(t, response["token"])

	// Verify user was created in database
	var user models.User
	err = srv.DB.Where("email = ?", "john.doe@gmail.com").First(&user).Error
	require.NoError(t, err)
	assert.Equal(t, "John", user.FirstName)
	assert.Equal(t, "Doe", user.LastName)
	assert.NotNil(t, user.TeamID)
	assert.True(t, user.IsAdmin)
}

func TestManualSignIn_Success(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	// Create user
	createTestUser(t, srv.DB, "test@gmail.com", "Test", "User", "password123", true)

	// Sign in
	signInReq := map[string]interface{}{
		"email":    "test@gmail.com",
		"password": "password123",
	}

	body, _ := json.Marshal(signInReq)
	req := httptest.NewRequest(http.MethodPost, "/api/sign-in", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)

	assert.Equal(t, http.StatusOK, rec.Code)

	var response map[string]string
	err := json.Unmarshal(rec.Body.Bytes(), &response)
	require.NoError(t, err)
	assert.NotEmpty(t, response["token"])
}

// createTestTeam is a helper to create a team in the test database
func createTestTeam(t *testing.T, db *gorm.DB, name string) *models.Team {
	team := &models.Team{
		Name: name,
	}
	err := db.Create(team).Error
	require.NoError(t, err)
	return team
}

// getJWTToken is a helper to get a JWT token for a user
func getJWTToken(t *testing.T, srv *server.Server, email string) string {
	token, err := srv.JwtIssuer.GenerateToken(email)
	require.NoError(t, err)
	return token
}

// TestSendTeamInvites_NewUser tests sending invites to new users
// New users should be able to sign up with the invite UUID and join the team
func TestSendTeamInvites_NewUser(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	// Create a team and admin user
	team := createTestTeam(t, srv.DB, "Test Team")
	adminUser := createTestUser(t, srv.DB, "admin@example.com", "Admin", "User", "password123", true)
	adminUser.TeamID = &team.ID
	err := srv.DB.Save(adminUser).Error
	require.NoError(t, err)

	// Get JWT token for admin user
	token := getJWTToken(t, srv, adminUser.Email)

	// Send invite to a new user email
	inviteReq := map[string]interface{}{
		"invitees": []string{"newuser@gmail.com"},
	}

	body, err := json.Marshal(inviteReq)
	require.NoError(t, err)

	req := httptest.NewRequest(http.MethodPost, "/api/auth/send-team-invites", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)
	rec := httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)

	// Assert invite was sent successfully
	assert.Equal(t, http.StatusOK, rec.Code)

	// Verify team invitation was created
	var teamInvitation models.TeamInvitation
	err = srv.DB.Where("team_id = ?", team.ID).First(&teamInvitation).Error
	require.NoError(t, err)
	assert.NotEmpty(t, teamInvitation.UniqueID)

	// Verify email invitation was created
	var emailInvitation models.EmailInvitation
	err = srv.DB.Where("email = ?", "newuser@gmail.com").First(&emailInvitation).Error
	require.NoError(t, err)
	assert.Equal(t, int(team.ID), emailInvitation.TeamID)

	// Test that new user can sign up with the invite UUID
	signUpReq := map[string]interface{}{
		"first_name":       "New",
		"last_name":        "User",
		"email":            "newuser@gmail.com",
		"password":         "securepassword123",
		"team_invite_uuid": teamInvitation.UniqueID,
	}

	body, err = json.Marshal(signUpReq)
	require.NoError(t, err)

	req = httptest.NewRequest(http.MethodPost, "/api/sign-up", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec = httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)
	assert.Equal(t, http.StatusCreated, rec.Code)

	// Verify new user was created and joined the team
	var newUser models.User
	err = srv.DB.Where("email = ?", "newuser@gmail.com").First(&newUser).Error
	require.NoError(t, err)
	assert.Equal(t, "New", newUser.FirstName)
	assert.Equal(t, "User", newUser.LastName)
	assert.NotNil(t, newUser.TeamID)
	assert.Equal(t, team.ID, *newUser.TeamID)
	assert.False(t, newUser.IsAdmin) // New users joining via invite are not admins
}

// TestSendTeamInvites_ExistingUserNoTeammates tests sending invites to existing users with no teammates
// Existing users should be able to use ChangeTeam to join the new team
func TestSendTeamInvites_ExistingUserNoTeammates(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	// Create first team and admin user
	team1 := createTestTeam(t, srv.DB, "Team 1")
	adminUser := createTestUser(t, srv.DB, "admin@example.com", "Admin", "User", "password123", true)
	adminUser.TeamID = &team1.ID
	err := srv.DB.Save(adminUser).Error
	require.NoError(t, err)

	// Create second team and existing user (with no teammates)
	team2 := createTestTeam(t, srv.DB, "Team 2")
	existingUser := createTestUser(t, srv.DB, "existing@example.com", "Existing", "User", "password123", true)
	existingUser.TeamID = &team2.ID
	err = srv.DB.Save(existingUser).Error
	require.NoError(t, err)

	// Get JWT token for admin user
	adminToken := getJWTToken(t, srv, adminUser.Email)

	// Send invite to existing user
	inviteReq := map[string]interface{}{
		"invitees": []string{"existing@example.com"},
	}

	body, err := json.Marshal(inviteReq)
	require.NoError(t, err)

	req := httptest.NewRequest(http.MethodPost, "/api/auth/send-team-invites", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+adminToken)
	rec := httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)
	assert.Equal(t, http.StatusOK, rec.Code)

	// Get team invitation UUID
	var teamInvitation models.TeamInvitation
	err = srv.DB.Where("team_id = ?", team1.ID).First(&teamInvitation).Error
	require.NoError(t, err)

	// Get JWT token for existing user
	existingToken := getJWTToken(t, srv, existingUser.Email)

	// Test that existing user can change teams using the invite UUID
	req = httptest.NewRequest(http.MethodPost, "/api/auth/change-team/"+teamInvitation.UniqueID, nil)
	req.Header.Set("Authorization", "Bearer "+existingToken)
	rec = httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)

	// Assert team change was successful
	assert.Equal(t, http.StatusOK, rec.Code)

	var response map[string]interface{}
	err = json.Unmarshal(rec.Body.Bytes(), &response)
	require.NoError(t, err)
	assert.Equal(t, "Successfully changed team", response["message"])
	assert.Equal(t, "Team 1", response["team_name"])

	// Verify user's team was updated
	err = srv.DB.Where("id = ?", existingUser.ID).First(&existingUser).Error
	require.NoError(t, err)
	assert.Equal(t, team1.ID, *existingUser.TeamID)
	assert.False(t, existingUser.IsAdmin) // User is no longer admin after joining new team
}

// TestSendTeamInvites_ExistingUserWithTeammates tests sending invites to existing users with teammates
// Existing users with teammates should NOT be able to change teams
func TestSendTeamInvites_ExistingUserWithTeammates(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	// Create first team and admin user
	team1 := createTestTeam(t, srv.DB, "Team 1")
	adminUser := createTestUser(t, srv.DB, "admin@example.com", "Admin", "User", "password123", true)
	adminUser.TeamID = &team1.ID
	err := srv.DB.Save(adminUser).Error
	require.NoError(t, err)

	// Create second team with admin user and a teammate
	team2 := createTestTeam(t, srv.DB, "Team 2")
	team2Admin := createTestUser(t, srv.DB, "team2admin@example.com", "Team2", "Admin", "password123", true)
	team2Admin.TeamID = &team2.ID
	err = srv.DB.Save(team2Admin).Error
	require.NoError(t, err)

	// Create a teammate
	teammate := createTestUser(t, srv.DB, "teammate@example.com", "Team", "Mate", "password123", false)
	teammate.TeamID = &team2.ID
	err = srv.DB.Save(teammate).Error
	require.NoError(t, err)

	// Get JWT token for admin user
	adminToken := getJWTToken(t, srv, adminUser.Email)

	// Send invite to team2 admin (who has a teammate)
	inviteReq := map[string]interface{}{
		"invitees": []string{"team2admin@example.com"},
	}

	body, err := json.Marshal(inviteReq)
	require.NoError(t, err)

	req := httptest.NewRequest(http.MethodPost, "/api/auth/send-team-invites", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+adminToken)
	rec := httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)

	// Assert invite was sent successfully
	assert.Equal(t, http.StatusOK, rec.Code)

	// Get team invitation UUID
	var teamInvitation models.TeamInvitation
	err = srv.DB.Where("team_id = ?", team1.ID).First(&teamInvitation).Error
	require.NoError(t, err)

	// Get JWT token for team2 admin
	team2AdminToken := getJWTToken(t, srv, team2Admin.Email)

	// Test that team2 admin CANNOT change teams because they have teammates
	req = httptest.NewRequest(http.MethodPost, "/api/auth/change-team/"+teamInvitation.UniqueID, nil)
	req.Header.Set("Authorization", "Bearer "+team2AdminToken)
	rec = httptest.NewRecorder()

	srv.Echo.ServeHTTP(rec, req)

	// Assert team change was rejected
	assert.Equal(t, http.StatusConflict, rec.Code)

	var errorResponse map[string]interface{}
	err = json.Unmarshal(rec.Body.Bytes(), &errorResponse)
	require.NoError(t, err)
	assert.Contains(t, errorResponse["message"].(string), "Cannot change teams")
	assert.Contains(t, errorResponse["message"].(string), "teammate(s)")

	// Verify user's team was NOT changed
	err = srv.DB.Where("id = ?", team2Admin.ID).First(&team2Admin).Error
	require.NoError(t, err)
	assert.Equal(t, team2.ID, *team2Admin.TeamID) // Still in original team
	assert.True(t, team2Admin.IsAdmin)            // Still admin of original team
}

type MockSocialAuthProvider struct {
	User  goth.User
	Error error
}

func (m *MockSocialAuthProvider) CompleteUserAuth(res http.ResponseWriter, req *http.Request) (goth.User, error) {
	return m.User, m.Error
}

func TestSocialLoginCallback_NewUserWithInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	team := createTestTeam(t, srv.DB, "Test Team")
	inviteUUID, err := uuid.NewV7()
	require.NoError(t, err)

	invitation := models.TeamInvitation{
		TeamID:   int(team.ID),
		UniqueID: inviteUUID.String(),
	}
	err = srv.DB.Create(&invitation).Error
	require.NoError(t, err)

	mockProvider := &MockSocialAuthProvider{
		User: goth.User{
			Email:     "newuser@gmail.com",
			FirstName: "New",
			LastName:  "User",
			AvatarURL: "https://example.com/avatar.jpg",
		},
		Error: nil,
	}

	authHandler := handlers.NewAuthHandler(
		srv.DB,
		srv.Config,
		srv.JwtIssuer,
		srv.Redis,
		mockProvider,
	)
	authHandler.ServerState.EmailClient = srv.EmailClient

	srv.Echo.Router().Add(http.MethodGet, "/api/auth/social/:provider/callback", authHandler.SocialLoginCallback)

	req := httptest.NewRequest(http.MethodGet, "/api/auth/social/google/callback", nil)
	rec := httptest.NewRecorder()

	sess, _ := srv.Store.Get(req, "session")
	sess.Values["team_invite_uuid"] = inviteUUID.String()
	sess.Save(req, rec)

	req.Header.Set("Cookie", rec.Header().Get("Set-Cookie"))

	srv.Echo.ServeHTTP(rec, req)

	assert.Equal(t, http.StatusFound, rec.Code)
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")

	var user models.User
	err = srv.DB.Where("email = ?", "newuser@gmail.com").First(&user).Error
	require.NoError(t, err)
	assert.Equal(t, team.ID, *user.TeamID)
	assert.False(t, user.IsAdmin)
}

func TestSocialLoginCallback_ExistingUserWithInvite(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	team1 := createTestTeam(t, srv.DB, "Team 1")
	existingUser := createTestUser(t, srv.DB, "existing@gmail.com", "Existing", "User", "password123", true)
	existingUser.TeamID = &team1.ID
	err := srv.DB.Save(existingUser).Error
	require.NoError(t, err)

	team2 := createTestTeam(t, srv.DB, "Team 2")
	inviteUUID, err := uuid.NewV7()
	require.NoError(t, err)

	invitation := models.TeamInvitation{
		TeamID:   int(team2.ID),
		UniqueID: inviteUUID.String(),
	}
	err = srv.DB.Create(&invitation).Error
	require.NoError(t, err)

	mockProvider := &MockSocialAuthProvider{
		User: goth.User{
			Email:     "existing@gmail.com",
			FirstName: "Existing",
			LastName:  "User",
		},
		Error: nil,
	}

	authHandler := handlers.NewAuthHandler(
		srv.DB,
		srv.Config,
		srv.JwtIssuer,
		srv.Redis,
		mockProvider,
	)
	authHandler.ServerState.EmailClient = srv.EmailClient

	srv.Echo.Router().Add(http.MethodGet, "/api/auth/social/:provider/callback", authHandler.SocialLoginCallback)

	req := httptest.NewRequest(http.MethodGet, "/api/auth/social/google/callback", nil)
	rec := httptest.NewRecorder()

	sess, _ := srv.Store.Get(req, "session")
	sess.Values["team_invite_uuid"] = inviteUUID.String()
	sess.Save(req, rec)

	req.Header.Set("Cookie", rec.Header().Get("Set-Cookie"))

	srv.Echo.ServeHTTP(rec, req)

	err = srv.DB.Where("id = ?", existingUser.ID).First(&existingUser).Error
	require.NoError(t, err)
	assert.Equal(t, team2.ID, *existingUser.TeamID)
	assert.False(t, existingUser.IsAdmin)
}
