package handlers

import (
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/models"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/labstack/echo/v4"
	"github.com/livekit/protocol/auth"
	"gorm.io/gorm"
)

func getTeamInfoRawJSON(accessToken string) ([]byte, error) {
	// Create the request
	req, err := http.NewRequest("GET", "https://slack.com/api/team.info", nil)
	if err != nil {
		return nil, fmt.Errorf("creating request: %w", err)
	}

	// Add authorization header
	req.Header.Add("Authorization", "Bearer "+accessToken)

	// Make the request
	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("making request: %w", err)
	}
	defer resp.Body.Close()

	// Read the raw response
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("reading response: %w", err)
	}

	// Return the raw JSON string
	return body, nil
}

func getTeamMembersRawJSON(accessToken string) ([]byte, error) {
	// Create the request
	req, err := http.NewRequest("GET", "https://slack.com/api/users.list", nil)
	if err != nil {
		return nil, fmt.Errorf("creating request: %w", err)
	}

	// Add query parameters
	q := req.URL.Query()
	q.Add("limit", "1000")
	req.URL.RawQuery = q.Encode()

	// Add authorization header
	req.Header.Add("Authorization", "Bearer "+accessToken)

	// Make the request
	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("making request: %w", err)
	}
	defer resp.Body.Close()

	// Read the raw response
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("reading response: %w", err)
	}

	// Return the raw JSON string
	return body, nil
}

func generateLiveKitTokens(s *common.ServerState, roomName string, participant *models.User) (common.LivekitTokenSet, error) {
	// Create an access token (make sure these are loaded from your config)
	videoID := fmt.Sprintf("room:%s:%s:video", roomName, participant.ID)
	audioID := fmt.Sprintf("room:%s:%s:audio", roomName, participant.ID)
	cameraID := fmt.Sprintf("room:%s:%s:camera", roomName, participant.ID)

	video := auth.
		NewAccessToken(s.Config.Livekit.APIKey, s.Config.Livekit.Secret).
		SetIdentity(videoID).
		SetValidFor(24 * time.Hour).
		SetName(participant.GetDisplayName() + " " + "video").
		SetVideoGrant(&auth.VideoGrant{
			RoomJoin: true,
			Room:     roomName,
		})

	audio := auth.
		NewAccessToken(s.Config.Livekit.APIKey, s.Config.Livekit.Secret).
		SetIdentity(audioID).
		SetValidFor(24 * time.Hour).
		SetName(participant.GetDisplayName() + " " + "audio").
		SetVideoGrant(&auth.VideoGrant{
			RoomJoin:             true,
			Room:                 roomName,
			CanUpdateOwnMetadata: &[]bool{true}[0],
		}).
		SetAttributes(map[string]string{
			"av1Support": "false",
		})

	camera := auth.
		NewAccessToken(s.Config.Livekit.APIKey, s.Config.Livekit.Secret).
		SetIdentity(cameraID).
		SetValidFor(24 * time.Hour).
		SetName(participant.GetDisplayName() + " " + "camera").
		SetVideoGrant(&auth.VideoGrant{
			RoomJoin: true,
			Room:     roomName,
		})

	videoToken, err := video.ToJWT()
	if err != nil {
		return common.LivekitTokenSet{}, fmt.Errorf("creating video token: %w", err)
	}

	audioToken, err := audio.ToJWT()
	if err != nil {
		return common.LivekitTokenSet{}, fmt.Errorf("creating audio token: %w", err)
	}

	cameraToken, err := camera.ToJWT()
	if err != nil {
		return common.LivekitTokenSet{}, fmt.Errorf("creating camera token: %w", err)
	}

	// Return the tokens
	return common.LivekitTokenSet{
		VideoToken:  videoToken,
		AudioToken:  audioToken,
		CameraToken: cameraToken,
	}, nil
}

// GetAuthenticatedUser returns the authenticated user from the session
// Returns nil and false if the user is not authenticated or not found
func getAuthenticatedUserFromJWTCommon(c echo.Context, jwtIssuer common.JWTIssuer, db *gorm.DB) (*models.User, bool) {
	email, err := jwtIssuer.GetUserEmail(c)
	if err != nil {
		return nil, false
	}

	// Fetch user from database
	user, err := models.GetUserByEmail(db, email)
	if err != nil {
		return nil, false
	}

	return user, true
}

func (h *AuthHandler) getAuthenticatedUserFromJWT(c echo.Context) (*models.User, bool) {
	return getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
}

func (bh *BillingHandler) getAuthenticatedUserFromJWT(c echo.Context) (*models.User, bool) {
	return getAuthenticatedUserFromJWTCommon(c, bh.JwtIssuer, bh.DB)
}

// convertLivekitURLToHTTP converts a LiveKit WebSocket URL (wss:// or ws://) to an HTTP URL.
// Uses net/url for safe URL parsing instead of string concatenation.
func convertLivekitURLToHTTP(livekitURL string) (string, error) {
	parsed, err := url.Parse(livekitURL)
	if err != nil {
		return "", fmt.Errorf("failed to parse LiveKit URL: %w", err)
	}

	// Convert scheme: wss -> https, ws -> http
	switch parsed.Scheme {
	case "wss":
		parsed.Scheme = "https"
	case "ws":
		parsed.Scheme = "http"
	case "https", "http":
		// Already HTTP(S), no change needed
	default:
		return "", fmt.Errorf("unexpected LiveKit URL scheme: %s", parsed.Scheme)
	}

	return parsed.String(), nil
}

// extractUserIDFromIdentity extracts the user ID from a LiveKit participant identity.
// Identity format is: "room:<roomId>:<userId>:<trackType>" (e.g., "room:abc123:usr_xyz:audio")
// Returns an error if the identity doesn't match the expected format.
func extractUserIDFromIdentity(identity string) (string, error) {
	parts := strings.Split(identity, ":")
	if len(parts) >= 4 && parts[0] == "room" {
		return parts[2], nil
	}
	return "", fmt.Errorf("invalid identity format: %s", identity)
}

// checkUserHasAccess checks if a user has an active subscription or trial.
// Returns true if the user is a Pro subscriber or has an active trial, false otherwise.
// Returns an error if the subscription check fails.
func checkUserHasAccess(db *gorm.DB, user *models.User) (bool, error) {
	// If user has no team, they have no subscription or trial access
	if user.TeamID == nil {
		return false, nil
	}

	userWithSub, err := models.GetUserWithSubscription(db, user)
	if err != nil {
		return false, fmt.Errorf("failed to get user subscription: %w", err)
	}

	hasAccess := userWithSub.IsPro
	if !hasAccess && userWithSub.IsTrial && userWithSub.TrialEndsAt != nil {
		hasAccess = userWithSub.TrialEndsAt.After(time.Now())
	}

	return hasAccess, nil
}
