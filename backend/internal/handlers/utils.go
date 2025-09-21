package handlers

import (
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/models"
	"io"
	"net/http"
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
			RoomJoin: true,
			Room:     roomName,
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

func generateMeetRedirectToken(s *common.ServerState, roomName string, participant *models.User) (string, error) {
	audioID := fmt.Sprintf("room:%s:%s:audio", roomName, participant.ID)

	audio := auth.
		NewAccessToken(s.Config.Livekit.APIKey, s.Config.Livekit.Secret).
		SetIdentity(audioID).
		SetValidFor(3 * time.Hour).
		SetVideoGrant(&auth.VideoGrant{
			RoomJoin: true,
			Room:     roomName,
		})

	audioToken, err := audio.ToJWT()
	if err != nil {
		return "", fmt.Errorf("creating audio token: %w", err)
	}

	// Return the tokens
	return audioToken, nil
}

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

// GetAuthenticatedUser returns the authenticated user from the session
// Returns nil and false if the user is not authenticated or not found
func (h *AuthHandler) getAuthenticatedUserFromJWT(c echo.Context) (*models.User, bool) {
	return getAuthenticatedUserFromJWTCommon(c, h.JwtIssuer, h.DB)
}

func (bh *BillingHandler) getAuthenticatedUserFromJWT(c echo.Context) (*models.User, bool) {
	return getAuthenticatedUserFromJWTCommon(c, bh.JwtIssuer, bh.DB)
}
