package handlers

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/config"
	"hopp-backend/internal/models"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/labstack/echo/v4"
	"github.com/livekit/protocol/livekit"
	lksdk "github.com/livekit/server-sdk-go/v2"
	"github.com/redis/go-redis/v9"
	"github.com/slack-go/slack"
	"github.com/twitchtv/twirp"
	"gorm.io/gorm"
)

// SlackHandler handles Slack app integration endpoints.
type SlackHandler struct {
	common.ServerState
	logger echo.Logger
}

// NewSlackHandler creates a new SlackHandler.
func NewSlackHandler(db *gorm.DB, cfg *config.Config, jwt common.JWTIssuer, redis *redis.Client, logger echo.Logger) *SlackHandler {
	return &SlackHandler{
		ServerState: common.ServerState{
			DB:        db,
			Config:    cfg,
			JwtIssuer: jwt,
			Redis:     redis,
		},
		logger: logger,
	}
}

// getBotTokenForSlackTeam retrieves and decrypts the bot token for a Slack workspace.
// This is a utility function used by participant add/remove operations.
func (h *SlackHandler) getBotTokenForSlackTeam(slackTeamID string) (string, error) {
	installation, err := models.GetSlackInstallationByTeamID(h.DB, slackTeamID)
	if err != nil {
		return "", fmt.Errorf("failed to get Slack installation: %w", err)
	}
	if installation == nil {
		return "", fmt.Errorf("slack installation not found for team %s", slackTeamID)
	}

	botToken, err := models.DecryptToken(installation.BotAccessToken, h.Config.SlackApp.TokenEncryptionKey)
	if err != nil {
		return "", fmt.Errorf("failed to decrypt bot token: %w", err)
	}

	return botToken, nil
}

// StartSlackRoomCleanup starts the background goroutine that cleans up empty Slack rooms.
// It checks LiveKit for actual participant count as the source of truth.
func (h *SlackHandler) StartSlackRoomCleanup() {
	h.logger.Info("Starting background cleanup goroutine")
	go func() {
		ticker := time.NewTicker(15 * time.Second)
		defer ticker.Stop()

		for range ticker.C {
			h.cleanupEmptySlackRooms()
		}
	}()
}

// cleanupEmptySlackRooms finds and deletes Slack rooms that have no participants in LiveKit.
// It uses LiveKit API as the source of truth for participant count.
// It waits for 5 minutes without participants before deleting a room.
func (h *SlackHandler) cleanupEmptySlackRooms() {
	// Find all Slack rooms (we'll check LiveKit for participant count)
	var rooms []models.Room
	result := h.DB.Where("type = ?", models.RoomTypeSlack).Find(&rooms)

	if result.Error != nil {
		h.logger.Errorf("could not fetch Slack rooms: %v", result.Error)
		return
	}

	if len(rooms) == 0 {
		return
	}

	// Create LiveKit room service client
	livekitHTTPURL, err := convertLivekitURLToHTTP(h.Config.Livekit.ServerURL)
	if err != nil {
		h.logger.Errorf("failed to convert LiveKit URL: %v", err)
		return
	}

	roomClient := lksdk.NewRoomServiceClient(livekitHTTPURL, h.Config.Livekit.APIKey, h.Config.Livekit.Secret)

	now := time.Now()
	threshold := now.Add(5 * -60 * time.Second)

	for i := range rooms {
		room := &rooms[i]

		// Create a fresh context with timeout for each room to avoid
		// a single slow/failed request from consuming the deadline for all rooms
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)

		// Check LiveKit for actual participant count
		participantCount, err := h.getLiveKitParticipantCount(ctx, roomClient, room.ID)
		cancel() // Cancel immediately after use to avoid leaking

		if err != nil {
			// Transient error (timeout, network issue, etc.), will skip this room
			// Don't clean up active rooms due to transient failures
			h.logger.Warnf("transient error getting participants for room %s, skipping cleanup: %v", room.ID, err)
			continue
		}

		if participantCount > 0 {
			// Room has participants - clear LastParticipantLeftAt if it was set
			if room.LastParticipantLeftAt != nil {
				room.LastParticipantLeftAt = nil
				h.DB.Save(room)
			}
			continue
		}

		// Room is empty - set or check LastParticipantLeftAt
		if room.LastParticipantLeftAt == nil {
			// First time seeing this room empty - set the timestamp
			room.LastParticipantLeftAt = &now
			h.DB.Save(room)
			continue
		}

		// Room has been empty - check if it's been empty long enough
		if room.LastParticipantLeftAt.Before(threshold) {
			h.cleanupSlackRoom(room)
		}
	}
}

// getLiveKitParticipantCount returns the number of unique participants in a LiveKit room.
// Returns other errors for transient failures (timeout, network issues, etc.).
func (h *SlackHandler) getLiveKitParticipantCount(ctx context.Context, roomClient *lksdk.RoomServiceClient, roomID string) (int, error) {
	participants, err := roomClient.ListParticipants(ctx, &livekit.ListParticipantsRequest{
		Room: roomID,
	})
	if err != nil {
		// Check if error is Twirp (not_found)
		if twrpErr, ok := err.(twirp.Error); ok {
			if twrpErr.Code() == twirp.NotFound {
				// Treat as an empty room
				return 0, nil
			}
		}

		// Return the error for transient failures
		// For non-existing rooms Livekit returns 0 participants
		return 0, fmt.Errorf("failed to list participants for room %s: %w", roomID, err)
	}

	// Dedupe participants by user ID (each user can have audio/video/camera identities)
	seenUserIDs := make(map[string]bool)
	for _, p := range participants.Participants {
		userID, err := extractUserIDFromIdentity(p.Identity)
		if err != nil {
			continue
		}
		seenUserIDs[userID] = true
	}

	return len(seenUserIDs), nil
}

// cleanupSlackRoom ends the Slack call and deletes the room.
func (h *SlackHandler) cleanupSlackRoom(room *models.Room) {
	slackMeta := room.GetSlackMetadata()
	if slackMeta != nil && slackMeta.SlackCallID != "" && slackMeta.SlackTeamID != "" {
		// Get bot token to end the call
		installation, err := models.GetSlackInstallationByTeamID(h.DB, slackMeta.SlackTeamID)
		if err == nil && installation != nil {
			botToken, err := models.DecryptToken(installation.BotAccessToken, h.Config.SlackApp.TokenEncryptionKey)
			if err == nil {
				if err := endSlackCall(botToken, slackMeta.SlackCallID); err != nil {
					h.logger.Warnf("failed to end Slack call %s: %v", slackMeta.SlackCallID, err)
				} else {
					h.logger.Infof("ended Slack call %s for room %s", slackMeta.SlackCallID, room.ID)
				}
			}
		}
	}

	// Delete the room
	if err := h.DB.Delete(room).Error; err != nil {
		h.logger.Errorf("failed to delete room %s: %v", room.ID, err)
	} else {
		h.logger.Infof("deleted empty Slack room %s", room.ID)
	}
}

// verifySlackRequest verifies the Slack request signature using the SDK's SecretsVerifier.
// It reads the request body, verifies the signature, and restores the body for further processing.
// Returns an error if verification fails.
func verifySlackRequest(c echo.Context, signingSecret string) error {
	body, err := io.ReadAll(c.Request().Body)
	if err != nil {
		return fmt.Errorf("failed to read request body: %w", err)
	}
	c.Request().Body = io.NopCloser(bytes.NewReader(body))

	sv, err := slack.NewSecretsVerifier(c.Request().Header, signingSecret)
	if err != nil {
		return fmt.Errorf("failed to create secrets verifier: %w", err)
	}
	if _, err := sv.Write(body); err != nil {
		return fmt.Errorf("failed to write body to Slack verifier: %w", err)
	}
	if err := sv.Ensure(); err != nil {
		return fmt.Errorf("invalid Slack signature: %w", err)
	}
	return nil
}

// SlackInstall redirects to Slack OAuth v2 authorize URL for app installation.
// This is an authenticated endpoint - the user's ID is stored in Redis and passed
// via the OAuth state parameter so we can link the installation to the user on callback.
// Accepts token via Authorization header OR query parameter (for browser redirects).
func (h *SlackHandler) SlackInstall(c echo.Context) error {
	if h.Config.SlackApp.ClientID == "" {
		return echo.NewHTTPError(http.StatusServiceUnavailable, "Slack app not configured")
	}

	// Get authenticated user (JWT middleware supports both header and query param)
	user, isAuthenticated := getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
	if !isAuthenticated || user == nil {
		return echo.NewHTTPError(http.StatusUnauthorized, "Authentication required")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "You must be part of a team to install Slack integration")
	}

	// Generate a cryptographically secure random state token
	stateToken := uuid.NewString()
	stateData := fmt.Sprintf("%s:%d", user.ID, *user.TeamID)

	// Store state -> user mapping in Redis (expires in 15 minutes)
	ctx := context.Background()
	if err := h.Redis.Set(ctx, stateToken, stateData, 15*time.Minute).Err(); err != nil {
		c.Logger().Errorf("Failed to store Slack install state: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to initiate installation")
	}

	authorizeURL, _ := url.Parse("https://slack.com/oauth/v2/authorize")
	q := authorizeURL.Query()
	q.Set("client_id", h.Config.SlackApp.ClientID)
	q.Set("scope", "commands,chat:write,chat:write.public,users:read,users:read.email,calls:read,calls:write")
	q.Set("redirect_uri", h.Config.SlackApp.RedirectURL)
	q.Set("state", stateToken)
	authorizeURL.RawQuery = q.Encode()

	return c.Redirect(http.StatusFound, authorizeURL.String())
}

// SlackOAuthCallback handles the OAuth callback from Slack app installation.
func (h *SlackHandler) SlackOAuthCallback(c echo.Context) error {
	code := c.QueryParam("code")
	if code == "" {
		return echo.NewHTTPError(http.StatusBadRequest, fmt.Sprintf("Slack OAuth error: %s", c.QueryParam("error")))
	}

	// Retrieve user info from state parameter (stored in Redis during SlackInstall)
	stateToken := c.QueryParam("state")
	var installedByID string
	var hoppTeamID *uint

	if stateToken == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Missing OAuth state")
	}

	ctx := context.Background()
	stateData, err := h.Redis.Get(ctx, stateToken).Result()

	if err != nil || stateData == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid or expired OAuth state")
	}

	// Parse "userID:teamID" format
	parts := strings.Split(stateData, ":")
	if len(parts) == 2 {
		installedByID = parts[0]
		if teamIDVal, err := strconv.ParseUint(parts[1], 10, 32); err == nil {
			teamID := uint(teamIDVal)
			hoppTeamID = &teamID
		}
	}
	// Clean up the state from Redis
	h.Redis.Del(ctx, stateToken)

	// Exchange code for token using slack-go
	resp, err := slack.GetOAuthV2Response(http.DefaultClient, h.Config.SlackApp.ClientID, h.Config.SlackApp.ClientSecret, code, h.Config.SlackApp.RedirectURL)
	if err != nil {
		c.Logger().Errorf("Failed to exchange Slack code: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to complete installation")
	}

	slackTeamID := resp.Team.ID
	teamName := resp.Team.Name
	botToken := resp.AccessToken
	botUserID := resp.BotUserID
	scopes := resp.Scope

	if slackTeamID == "" || botToken == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid OAuth response from Slack")
	}

	encryptedToken, err := models.EncryptToken(botToken, h.Config.SlackApp.TokenEncryptionKey)
	if err != nil {
		c.Logger().Errorf("Failed to encrypt bot token: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to store installation")
	}

	// Upsert installation
	var installation models.SlackInstallation
	result := h.DB.Where("slack_team_id = ?", slackTeamID).First(&installation)

	if result.Error == gorm.ErrRecordNotFound {
		installation = models.SlackInstallation{
			SlackTeamID:    slackTeamID,
			SlackTeamName:  teamName,
			BotAccessToken: encryptedToken,
			BotUserID:      botUserID,
			Scopes:         scopes,
			InstalledAt:    time.Now(),
			InstalledByID:  installedByID,
			TeamID:         hoppTeamID,
		}
		h.DB.Create(&installation)
	} else {
		installation.SlackTeamName = teamName
		installation.BotAccessToken = encryptedToken
		installation.BotUserID = botUserID
		installation.Scopes = scopes
		installation.InstalledAt = time.Now()
		if installedByID != "" {
			installation.InstalledByID = installedByID
		}
		if hoppTeamID != nil {
			installation.TeamID = hoppTeamID
		}
		h.DB.Save(&installation)
	}

	c.Logger().Infof("Slack app installed for team %s (%s)", teamName, slackTeamID)

	// Redirect to web-app success page
	successURL := fmt.Sprintf("https://%s/integrations/slack/success", h.Config.Server.DeployDomain)
	return c.Redirect(http.StatusFound, successURL)
}

// GetSlackInstallation handles GET /api/auth/slack/installation
// Returns the Slack installation for the current user's team.
func (h *SlackHandler) GetSlackInstallation(c echo.Context) error {
	user, isAuthenticated := getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Authentication required")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User is not part of a team")
	}

	installation, err := models.GetSlackInstallationByHoppTeamID(h.DB, *user.TeamID)
	if err != nil {
		c.Logger().Errorf("Failed to get Slack installation: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get Slack installation")
	}

	if installation == nil {
		return echo.NewHTTPError(http.StatusNotFound, "No Slack installation found for this team")
	}

	return c.JSON(http.StatusOK, installation)
}

// DeleteSlackInstallation handles DELETE /api/auth/slack/installation
// Deletes the Slack installation for the current user's team.
// Only admins can delete the installation.
func (h *SlackHandler) DeleteSlackInstallation(c echo.Context) error {
	user, isAuthenticated := getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Authentication required")
	}

	if !user.IsAdmin {
		return echo.NewHTTPError(http.StatusForbidden, "Only admins can delete Slack installations")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User is not part of a team")
	}

	installation, err := models.GetSlackInstallationByHoppTeamID(h.DB, *user.TeamID)
	if err != nil {
		c.Logger().Errorf("Failed to get Slack installation: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get Slack installation")
	}

	if installation == nil {
		return echo.NewHTTPError(http.StatusNotFound, "No Slack installation found for this team")
	}

	if err := models.DeleteSlackInstallation(h.DB, installation.ID); err != nil {
		c.Logger().Errorf("Failed to delete Slack installation: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to delete Slack installation")
	}

	c.Logger().Infof("Deleted Slack installation %d for team %d by user %s", installation.ID, *user.TeamID, user.ID)

	return c.NoContent(http.StatusNoContent)
}

// HandleHoppCommand handles the /hopp slash command from Slack.
// Creates a temporary Room with type=slack and stores Slack metadata.
func (h *SlackHandler) HandleHoppCommand(c echo.Context) error {
	if err := verifySlackRequest(c, h.Config.SlackApp.SigningSecret); err != nil {
		c.Logger().Warnf("Slack signature verification failed: %v", err)
		return echo.NewHTTPError(http.StatusUnauthorized, "Invalid request signature")
	}

	if err := c.Request().ParseForm(); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, "Failed to parse form")
	}

	teamID := c.FormValue("team_id")
	channelID := c.FormValue("channel_id")
	slackUserID := c.FormValue("user_id")
	userName := c.FormValue("user_name")

	installation, err := models.GetSlackInstallationByTeamID(h.DB, teamID)
	if err != nil || installation == nil {
		return c.String(http.StatusOK, "Error: Hopp is not installed in this workspace. Please install it first.")
	}

	// Use Slack username as room name
	roomName := fmt.Sprintf("%s's Hopp Session", userName)

	// Create a temporary Room with type=slack, linked to the same team as the Slack installation
	room := models.Room{
		Name:   roomName,
		UserID: slackUserID, // Use Slack user ID as creator (no Hopp user required)
		Type:   models.RoomTypeSlack,
		Temp:   true,
		TeamID: installation.TeamID, // Link to the Hopp team that installed Slack
	}

	if err := h.DB.Create(&room).Error; err != nil {
		c.Logger().Errorf("Failed to create room: %v", err)
		return c.String(http.StatusOK, "Error: Failed to create pairing session.")
	}

	joinURL := fmt.Sprintf("https://%s/api/slack/join/%s", h.Config.Server.DeployDomain, room.ID)

	botToken, err := models.DecryptToken(installation.BotAccessToken, h.Config.SlackApp.TokenEncryptionKey)
	if err != nil {
		return c.String(http.StatusOK, "Error: Failed to post message.")
	}

	// Use Slack's Calls API to create a native call card
	// NOTE: created_by must be a Slack user ID (not a display name).
	call, err := h.createSlackCall(botToken, room.ID, slackUserID, userName, joinURL, channelID)
	if err != nil {
		c.Logger().Errorf("Failed to create Slack call: %v", err)
		// Delete the room since we failed to create the Slack call
		h.DB.Delete(&room)
		return c.String(http.StatusOK, "Error: Failed to create pairing session. Please try again.")
	}

	// Prepare Slack metadata
	slackMeta := &models.SlackMetadata{
		SlackTeamID:    teamID,
		SlackChannelID: channelID,
		SlackCallID:    call.ID,
		SlackMessageTS: call.ID,
	}

	// Store Slack metadata in the room
	if err := room.SetSlackMetadata(slackMeta); err != nil {
		c.Logger().Warnf("Failed to set Slack metadata: %v", err)
	}
	h.DB.Save(&room)

	c.Logger().Infof("Created Slack room %s with call %s for %s in channel %s", room.ID, slackMeta.SlackCallID, userName, channelID)
	return c.String(http.StatusOK, "")
}

// HandleInteraction handles interactive component callbacks from Slack.
// This is called when users click buttons or interact with message components.
func (h *SlackHandler) HandleInteraction(c echo.Context) error {
	if err := verifySlackRequest(c, h.Config.SlackApp.SigningSecret); err != nil {
		c.Logger().Warnf("Slack signature verification failed for interaction: %v", err)
		return echo.NewHTTPError(http.StatusUnauthorized, "Invalid request signature")
	}

	// Parse the payload
	payload := c.FormValue("payload")
	if payload == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Missing payload")
	}

	var interaction slack.InteractionCallback
	if err := json.Unmarshal([]byte(payload), &interaction); err != nil {
		c.Logger().Errorf("Failed to parse interaction payload: %v", err)
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid payload")
	}

	c.Logger().Infof("Received Slack interaction: type=%s, user=%s", interaction.Type, interaction.User.ID)

	// For URL buttons, Slack just needs a 200 OK - the URL handles the action
	// For other button types, we could handle them here
	return c.String(http.StatusOK, "")
}

// GetSessionTokens handles GET /api/auth/slack/session/:sessionId/tokens
// This endpoint returns LiveKit tokens for joining a Slack room.
// It's called by the desktop app when handling a join-session deep-link.
func (h *SlackHandler) GetSessionTokens(c echo.Context) error {
	sessionID := c.Param("sessionId")

	// Authenticate the user
	user, isAuthenticated := getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Authentication required")
	}

	// Get the room
	room, err := models.GetRoomByID(h.DB, sessionID)
	if err != nil {
		c.Logger().Errorf("Failed to get room: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to load session")
	}
	if room == nil {
		return echo.NewHTTPError(http.StatusNotFound, "Session not found")
	}

	if room.TeamID == nil || user.TeamID == nil || *room.TeamID != *user.TeamID {
		c.Logger().Warnf("User %s (team %v) attempted to join Slack session from team %v",
			user.ID, user.TeamID, room.TeamID)
		return echo.NewHTTPError(http.StatusForbidden, "You don't have access to this session")
	}

	// Check if user has access (paid or active trial)
	userWithSub, err2 := models.GetUserWithSubscription(h.DB, user)
	if err2 != nil {
		c.Logger().Error("Error getting user subscription: ", err2)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to check subscription status")
	}

	hasAccess := userWithSub.IsPro
	if !hasAccess && userWithSub.IsTrial && userWithSub.TrialEndsAt != nil {
		hasAccess = userWithSub.TrialEndsAt.After(time.Now())
	}

	if !hasAccess {
		return c.JSON(http.StatusForbidden, map[string]string{"error": "trial-ended"})
	}

	// Generate LiveKit tokens for this room
	tokens, err := generateLiveKitTokens(&h.ServerState, room.ID, user)
	if err != nil {
		c.Logger().Errorf("Failed to generate tokens for session: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to generate tokens")
	}

	// For Slack rooms, add user as participant to the Slack call
	if room.Type == models.RoomTypeSlack {
		slackMeta := room.GetSlackMetadata()
		if slackMeta != nil && slackMeta.SlackCallID != "" && slackMeta.SlackTeamID != "" {
			// Run async so we don't block the token response
			go func() {
				botToken, err := h.getBotTokenForSlackTeam(slackMeta.SlackTeamID)
				if err != nil {
					h.logger.Warnf("Failed to get bot token for participant update: %v", err)
					return
				}
				if err := addParticipantToSlackCall(botToken, slackMeta.SlackCallID, user); err != nil {
					h.logger.Warnf("Failed to add participant to Slack call: %v", err)
				} else {
					h.logger.Infof("Added user %s as participant to Slack call %s", user.ID, slackMeta.SlackCallID)
				}
			}()
		} else {
			h.logger.Warnf("Failed to get Slack metadata for participant update (room %s)", room.ID)
		}
	}

	// Clear LastParticipantLeftAt since someone is joining
	if room.LastParticipantLeftAt != nil {
		room.LastParticipantLeftAt = nil
		h.DB.Save(room)
	}

	c.Logger().Infof("Generated tokens for user %s joining session %s", user.ID, sessionID)

	return c.JSON(http.StatusOK, tokens)
}

// JoinPairingSession handles GET /api/slack/join/:sessionId
// This endpoint validates the room exists and redirects to the web-app's
// /slack/join/:sessionId route, which handles authentication and deep-links
// to the desktop app with the necessary tokens.
func (h *SlackHandler) JoinPairingSession(c echo.Context) error {
	sessionID := c.Param("sessionId")

	room, err := models.GetRoomByID(h.DB, sessionID)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to load session")
	}
	if room == nil {
		return echo.NewHTTPError(http.StatusNotFound, "Session not found")
	}

	// Redirect to the web-app's Slack join page
	// The web-app will handle authentication and deep-link to the desktop app
	webAppURL := fmt.Sprintf("https://%s/slack/join/%s", h.Config.Server.DeployDomain, room.ID)
	return c.Redirect(http.StatusFound, webAppURL)
}

// SlackCall represents a Slack call object
type SlackCall struct {
	ID string `json:"id"`
}

// newSlackClient creates a Slack client with a sensible HTTP timeout.
// This prevents potential hangs from Slack API calls.
func newSlackClient(botToken string) *slack.Client {
	httpClient := &http.Client{
		Timeout: 10 * time.Second,
	}
	return slack.New(botToken, slack.OptionHTTPClient(httpClient))
}

// lookupSlackUserByEmail looks up a Slack user by email using the users.lookupByEmail API.
// Returns the Slack user ID if found, or empty string if not found.
func lookupSlackUserByEmail(botToken, email string) (string, error) {
	api := newSlackClient(botToken)
	user, err := api.GetUserByEmail(email)
	if err != nil {
		return "", err
	}
	return user.ID, nil
}

// addParticipantToSlackCall adds a user as a participant to a Slack call.
// It first tries to look up the user's Slack ID by email. If found, uses the native
// Slack user format (which enables automatic status updates). Falls back to external
// user format if the email lookup fails.
func addParticipantToSlackCall(botToken, callID string, user *models.User) error {
	api := newSlackClient(botToken)

	var participant slack.CallParticipant

	// Try to look up the Slack user by their Hopp email
	slackUserID, lookupErr := lookupSlackUserByEmail(botToken, user.Email)
	if lookupErr == nil && slackUserID != "" {
		// Found the Slack user - use native format for automatic status updates
		participant = slack.CallParticipant{
			SlackID: slackUserID,
		}
	} else {
		// Slack user not found - fall back to external user format
		participant = slack.CallParticipant{
			ExternalID:  user.ID,
			DisplayName: user.GetDisplayName(),
		}
		if user.AvatarURL != "" {
			participant.AvatarURL = user.AvatarURL
		}
	}

	return api.CallAddParticipants(callID, []slack.CallParticipant{participant})
}

// createSlackCall creates a call using Slack's Calls API for native call UI
func (h *SlackHandler) createSlackCall(botToken, externalID, createdBySlackUserID, creatorName, joinURL, channelID string) (*SlackCall, error) {
	api := newSlackClient(botToken)

	// Create the call using the SDK
	call, err := api.AddCall(slack.AddCallParameters{
		JoinURL:          joinURL,
		ExternalUniqueID: externalID,
		CreatedBy:        createdBySlackUserID,
		Title:            fmt.Sprintf("%s started a Hopp pairing session", creatorName),
	})
	if err != nil {
		return nil, fmt.Errorf("calls.add error: %w", err)
	}

	// Post the call to the channel using the SDK's CallBlock
	_, _, err = api.PostMessage(channelID,
		slack.MsgOptionText(fmt.Sprintf("%s started a Hopp pairing session", creatorName), false),
		slack.MsgOptionBlocks(slack.NewCallBlock(call.ID)),
	)
	if err != nil {
		h.logger.Warnf("Failed to post call block: %v", err)
	}

	return &SlackCall{ID: call.ID}, nil
}

// removeParticipantFromSlackCall removes a user from a Slack call.
// It uses the user's email to look up their Slack ID, falling back to external_id.
func removeParticipantFromSlackCall(botToken, callID string, user *models.User) error {
	api := newSlackClient(botToken)

	var participant slack.CallParticipant

	// Try to look up the Slack user by email
	slackUserID, lookupErr := lookupSlackUserByEmail(botToken, user.Email)
	if lookupErr == nil && slackUserID != "" {
		participant = slack.CallParticipant{
			SlackID: slackUserID,
		}
	} else {
		// Fall back to external user format
		// Note: DisplayName is required alongside ExternalID for the Slack API
		participant = slack.CallParticipant{
			ExternalID:  user.ID,
			DisplayName: user.GetDisplayName(),
		}
	}

	return api.CallRemoveParticipants(callID, []slack.CallParticipant{participant})
}

// endSlackCall ends a Slack call using the calls.end API.
func endSlackCall(botToken, callID string) error {
	api := newSlackClient(botToken)
	return api.EndCall(callID, slack.EndCallParameters{})
}

// LeaveRoom handles POST /api/auth/room/:id/leave
// This endpoint is called when a user leaves a room.
// For Slack rooms, it removes the user from the Slack call and updates LastParticipantLeftAt.
// This could move to the regular handlers, but for now we only use it for Slack rooms, will revisit.
func (h *SlackHandler) LeaveRoom(c echo.Context) error {
	roomID := c.Param("id")

	// Authenticate the user
	user, isAuthenticated := getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Authentication required")
	}

	// Get the room
	room, err := models.GetRoomByID(h.DB, roomID)
	if err != nil {
		c.Logger().Errorf("Failed to get room: %v", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to load room")
	}
	if room == nil {
		return echo.NewHTTPError(http.StatusNotFound, "Room not found")
	}

	// For Slack rooms, remove the user from the Slack call
	if room.Type == models.RoomTypeSlack {
		h.logger.Infof("leaving Slack room: %s - User %s", roomID, user.ID)
		slackMeta := room.GetSlackMetadata()
		if slackMeta != nil && slackMeta.SlackCallID != "" && slackMeta.SlackTeamID != "" {
			// Run async so we don't block the leave response
			go func() {
				botToken, err := h.getBotTokenForSlackTeam(slackMeta.SlackTeamID)
				if err != nil {
					h.logger.Warnf("Failed to get bot token for participant removal: %v", err)
					return
				}
				if err := removeParticipantFromSlackCall(botToken, slackMeta.SlackCallID, user); err != nil {
					h.logger.Warnf("Failed to remove participant from Slack call: %v", err)
				} else {
					h.logger.Infof("Removed user %s from Slack call %s", user.ID, slackMeta.SlackCallID)
				}
			}()
		} else {
			h.logger.Warnf("Failed to get Slack metadata for participant removal for team %s", user.Team)
		}
	}

	// Update LastParticipantLeftAt
	now := time.Now()
	room.LastParticipantLeftAt = &now
	if err := h.DB.Save(room).Error; err != nil {
		c.Logger().Warnf("Failed to update room LastParticipantLeftAt: %v", err)
	}

	c.Logger().Infof("User %s left room %s", user.ID, roomID)

	return c.NoContent(http.StatusOK)
}
