//go:build integration
// +build integration

package integration

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/config"
	"hopp-backend/internal/models"
	"hopp-backend/internal/server"

	"github.com/labstack/gommon/log"
	"gorm.io/gorm"
)

// preCutoffDate is comfortably before the hard-paywall cutoff so teams stamped
// with it keep the legacy free trial.
var preCutoffDate = time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)

// postCutoffDate is comfortably after the hard-paywall cutoff so teams stamped
// with it are always subject to the card-required paywall, regardless of when
// the suite runs or where the cutoff is moved.
var postCutoffDate = time.Date(2100, 1, 1, 0, 0, 0, 0, time.UTC)

// setupTestServerWithStripe builds a test server with Stripe "enabled" (a dummy
// secret key) so the access guard in GetUserWithSubscription is exercised. No
// real Stripe calls are made on the 402 path.
func setupTestServerWithStripe(t *testing.T) (*server.Server, func()) {
	cfg := &config.Config{}
	cfg.Server.Port = "8080"
	cfg.Server.Host = "localhost"
	cfg.Server.DeployDomain = "localhost:8080"
	cfg.Server.Debug = false
	cfg.Database.DSN = "file::memory:?cache=shared"
	cfg.Database.RedisURI = ""
	cfg.Auth.SessionSecret = "test-secret-key-for-testing-only"
	cfg.Resend.DefaultSender = "test@example.com"
	cfg.Stripe.SecretKey = "sk_test_dummy"
	cfg.Stripe.TrialPeriodDays = 14

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

// setTeamCreatedAt forces a team's created_at (gorm stamps it on insert).
func setTeamCreatedAt(t *testing.T, db *gorm.DB, teamID uint, createdAt time.Time) {
	err := db.Model(&models.Team{}).Where("id = ?", teamID).Update("created_at", createdAt).Error
	require.NoError(t, err)
}

// makeSubscription inserts a Stripe subscription with the given status for a team.
func makeSubscription(t *testing.T, db *gorm.DB, teamID uint, status models.SubscriptionStatus) {
	sub := &models.Subscription{
		TeamID:               teamID,
		StripeCustomerID:     "cus_test_" + string(status),
		StripeSubscriptionID: "sub_test_" + string(status) + "_" + time.Now().Format("150405.000000000"),
		Status:               status,
		Tier:                 models.TierPaid,
		BillingInterval:      models.IntervalMonthly,
		CurrentPeriodStart:   time.Now(),
		CurrentPeriodEnd:     time.Now().AddDate(0, 1, 0),
	}
	require.NoError(t, db.Create(sub).Error)
}

// newAdminWithTeam creates a team and an admin user assigned to it. The team's
// created_at is explicitly stamped to a known post-cutoff value so the paywall
// tests always exercise the hard-paywall path; callers override it (via
// setTeamCreatedAt) for legacy pre-cutoff cases.
func newAdminWithTeam(t *testing.T, srv *server.Server, teamName, email string) (*models.Team, *models.User) {
	team := createTestTeam(t, srv.DB, teamName)
	setTeamCreatedAt(t, srv.DB, team.ID, postCutoffDate)
	team.CreatedAt = postCutoffDate
	user := createTestUser(t, srv.DB, email, "Admin", "User", "password123", true)
	user.TeamID = &team.ID
	require.NoError(t, srv.DB.Save(user).Error)
	return team, user
}

// joinRoomCode creates a room for the team and returns the HTTP status of the
// user attempting to join it (the paywall-gated path).
func joinRoomCode(t *testing.T, srv *server.Server, user *models.User, teamID uint) int {
	room := &models.Room{Name: "room-" + user.ID, UserID: user.ID, TeamID: &teamID}
	require.NoError(t, srv.DB.Create(room).Error)

	req := httptest.NewRequest(http.MethodGet, "/api/auth/room/"+room.ID, nil)
	req.Header.Set("Authorization", "Bearer "+getJWTToken(t, srv, user.Email))
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	return rec.Code
}

// TestHardPaywall_PreCutoffTeam_KeepsFreeTrial verifies legacy teams created
// before the cutoff keep the CreatedAt-based free trial.
func TestHardPaywall_PreCutoffTeam_KeepsFreeTrial(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	team, user := newAdminWithTeam(t, srv, "Legacy Team", "legacy-admin@example.com")
	setTeamCreatedAt(t, srv.DB, team.ID, preCutoffDate)

	result, err := models.GetUserWithSubscription(srv.DB, user, true)
	require.NoError(t, err)

	assert.False(t, result.IsPro, "pre-cutoff team without sub is not pro")
	assert.True(t, result.IsTrial, "pre-cutoff team keeps the free trial")
	require.NotNil(t, result.TrialEndsAt)
	assert.Equal(t, preCutoffDate.AddDate(0, 0, 14), result.TrialEndsAt.UTC())
}

// TestHardPaywall_PostCutoffAccessByStatus verifies that, for post-cutoff teams,
// only active/trialing subscriptions grant access and the legacy free trial is
// never applied. An empty status means no subscription row at all.
func TestHardPaywall_PostCutoffAccessByStatus(t *testing.T) {
	cases := []struct {
		name    string
		status  models.SubscriptionStatus
		isPro   bool
		isTrial bool
	}{
		{"no subscription", "", false, false},
		{"trialing", models.StatusTrialing, true, true},
		{"active", models.StatusActive, true, false},
		{"canceled", models.StatusCanceled, false, false},
		{"past_due", models.StatusPastDue, false, false},
		{"incomplete", models.StatusIncomplete, false, false},
	}

	for i, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			srv, cleanup := setupTestServerFast(t)
			defer cleanup()

			team, user := newAdminWithTeam(t, srv, fmt.Sprintf("Access %d", i), fmt.Sprintf("access%d@example.com", i))
			if tc.status != "" {
				makeSubscription(t, srv.DB, team.ID, tc.status)
			}

			result, err := models.GetUserWithSubscription(srv.DB, user, true)
			require.NoError(t, err)

			assert.Equal(t, tc.isPro, result.IsPro)
			// The legacy CreatedAt-based free trial never applies post-cutoff;
			// only a card-on-file (trialing) subscription surfaces the trial
			// countdown, with its end date derived from the Stripe period.
			assert.Equal(t, tc.isTrial, result.IsTrial)
			if tc.isTrial {
				assert.NotNil(t, result.TrialEndsAt, "trialing sub exposes the trial end date")
			} else {
				assert.Nil(t, result.TrialEndsAt)
			}
		})
	}
}

// TestHardPaywall_InvitedUserInheritsAccess verifies a non-admin teammate of a
// post-cutoff team with an active subscription inherits access and never hits
// the card gate.
func TestHardPaywall_InvitedUserInheritsAccess(t *testing.T) {
	srv, cleanup := setupTestServerFast(t)
	defer cleanup()

	team, _ := newAdminWithTeam(t, srv, "Active Team", "active-admin@example.com")

	member := createTestUser(t, srv.DB, "member@example.com", "Team", "Member", "password123", false)
	member.TeamID = &team.ID
	require.NoError(t, srv.DB.Save(member).Error)

	makeSubscription(t, srv.DB, team.ID, models.StatusActive)

	result, err := models.GetUserWithSubscription(srv.DB, member, true)
	require.NoError(t, err)

	assert.True(t, result.IsPro, "invited member inherits team subscription access")
}

// TestHardPaywall_RoomJoinBlocked verifies the 402 paywall path blocks calls for
// post-cutoff teams that lack access: no subscription, canceled, or past_due.
// This is the end-to-end enforcement behind "cancel -> no more calls".
func TestHardPaywall_RoomJoinBlocked(t *testing.T) {
	cases := []struct {
		name   string
		status models.SubscriptionStatus
	}{
		{"no subscription", ""},
		{"canceled", models.StatusCanceled},
		{"past_due", models.StatusPastDue},
	}

	for i, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			srv, cleanup := setupTestServerWithStripe(t)
			defer cleanup()

			team, user := newAdminWithTeam(t, srv, fmt.Sprintf("Gated %d", i), fmt.Sprintf("gated%d@example.com", i))
			if tc.status != "" {
				makeSubscription(t, srv.DB, team.ID, tc.status)
			}

			code := joinRoomCode(t, srv, user, team.ID)
			assert.Equal(t, http.StatusPaymentRequired, code, "team without access is blocked at room join")
		})
	}
}

// subscriptionStatusResponse mirrors the GET /api/auth/billing/subscription body.
type subscriptionStatusResponse struct {
	Subscription struct {
		Status                string `json:"status"`
		ManualUpgrade         bool   `json:"manual_upgrade"`
		IsAdmin               bool   `json:"is_admin"`
		RequiresPaymentMethod bool   `json:"requires_payment_method"`
	} `json:"subscription"`
}

// getSubscriptionStatus calls the subscription status endpoint for a user.
func getSubscriptionStatus(t *testing.T, srv *server.Server, email string) subscriptionStatusResponse {
	token := getJWTToken(t, srv, email)
	req := httptest.NewRequest(http.MethodGet, "/api/auth/billing/subscription", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	require.Equal(t, http.StatusOK, rec.Code, "subscription status: %s", rec.Body.String())

	var resp subscriptionStatusResponse
	require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &resp))
	return resp
}

// TestHardPaywall_RequiresPaymentMethod verifies the onboarding gate flag. It is
// true only when a post-cutoff team has never set up billing (no subscription
// row). Trialing/active/canceled/past_due teams already have a row, manual
// upgrades are exempt, and pre-cutoff teams keep the legacy flow — so none of
// them are forced through onboarding.
func TestHardPaywall_RequiresPaymentMethod(t *testing.T) {
	cases := []struct {
		name     string
		setup    func(t *testing.T, srv *server.Server, team *models.Team)
		expected bool
	}{
		{"new post-cutoff team must onboard", func(t *testing.T, srv *server.Server, team *models.Team) {}, true},
		{"trialing team has a card", func(t *testing.T, srv *server.Server, team *models.Team) {
			makeSubscription(t, srv.DB, team.ID, models.StatusTrialing)
		}, false},
		{"active team", func(t *testing.T, srv *server.Server, team *models.Team) {
			makeSubscription(t, srv.DB, team.ID, models.StatusActive)
		}, false},
		{"canceled team re-subscribes from dashboard", func(t *testing.T, srv *server.Server, team *models.Team) {
			makeSubscription(t, srv.DB, team.ID, models.StatusCanceled)
		}, false},
		{"past_due team is not re-onboarded", func(t *testing.T, srv *server.Server, team *models.Team) {
			makeSubscription(t, srv.DB, team.ID, models.StatusPastDue)
		}, false},
		{"manual upgrade is exempt", func(t *testing.T, srv *server.Server, team *models.Team) {
			team.IsManualUpgrade = true
			require.NoError(t, srv.DB.Save(team).Error)
		}, false},
		{"pre-cutoff team keeps legacy flow", func(t *testing.T, srv *server.Server, team *models.Team) {
			setTeamCreatedAt(t, srv.DB, team.ID, preCutoffDate)
		}, false},
	}

	for i, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			srv, cleanup := setupTestServerWithStripe(t)
			defer cleanup()

			team, user := newAdminWithTeam(t, srv, fmt.Sprintf("RPM %d", i), fmt.Sprintf("rpm%d@example.com", i))
			tc.setup(t, srv, team)

			resp := getSubscriptionStatus(t, srv, user.Email)
			assert.Equal(t, tc.expected, resp.Subscription.RequiresPaymentMethod)

			// A blocked post-cutoff team (no subscription) must not be reported as
			// trialing, otherwise clients treat it as if a trial already exists.
			if tc.expected {
				assert.NotEqual(t, string(models.StatusTrialing), resp.Subscription.Status,
					"team requiring a payment method must not report trialing")
			}
		})
	}
}
