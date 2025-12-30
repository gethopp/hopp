package handlers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/config"
	"hopp-backend/internal/models"
	"hopp-backend/internal/notifications"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/labstack/echo-contrib/session"
	"github.com/labstack/echo/v4"
	"github.com/lindell/go-burner-email-providers/burner"
	"github.com/markbates/goth"
	"github.com/markbates/goth/gothic"
	"github.com/redis/go-redis/v9"
	"github.com/tidwall/gjson"
	"gorm.io/gorm"
)

type AuthHandler struct {
	common.ServerState
	SocialAuth common.SocialAuthProvider
}

type SignInRequest struct {
	Email    string `json:"email" validate:"required,email"`
	Password string `json:"password" validate:"required"`
}

type ForgotPasswordRequest struct {
	Email string `json:"email" validate:"required,email"`
}

type ResetPasswordRequest struct {
	Password string `json:"password" validate:"required"`
}

func NewAuthHandler(db *gorm.DB, cfg *config.Config, jwt common.JWTIssuer, redis *redis.Client, socialAuth common.SocialAuthProvider) *AuthHandler {
	return &AuthHandler{
		ServerState: common.ServerState{
			DB:        db,
			Config:    cfg,
			JwtIssuer: jwt,
			Redis:     redis,
		},
		SocialAuth: socialAuth,
	}
}

type RealGothicProvider struct{}

func (r *RealGothicProvider) CompleteUserAuth(res http.ResponseWriter, req *http.Request) (goth.User, error) {
	return gothic.CompleteUserAuth(res, req)
}

func (h *AuthHandler) SocialLoginCallback(c echo.Context) error {
	user, err := h.SocialAuth.CompleteUserAuth(c.Response(), c.Request())
	if err != nil {
		return err
	}

	if user.Email == "" {
		c.Logger().Error("User email is empty from provider")
		return echo.NewHTTPError(http.StatusBadRequest, "Email is required but not provided by the authentication provider")
	}

	var u models.User
	// Will be used to get Slack's team name in case its not an invite
	var teamName string
	providerName := c.Param("provider")
	isNewUser := false // Flag to track if a new user was created

	// Execute everything in a transaction
	err = h.DB.Transaction(func(tx *gorm.DB) error {
		// Check if user exists or not
		result := tx.Where("email = ?", user.Email).First(&u)

		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			isNewUser = true // Mark as new user

			var assignedTeamID *uint

			// Check if the user has a team invite UUID
			sess, err := session.Get("session", c)
			if err == nil {
				inviteUUID := sess.Values["team_invite_uuid"]
				if inviteUUID != nil {
					// Find team that this invitation belongs to
					var invitation models.TeamInvitation
					if err := tx.Where("unique_id = ?", inviteUUID).First(&invitation).Error; err == nil {
						teamID := uint(invitation.TeamID)
						assignedTeamID = &teamID
					}
				}
				// Clean up the session
				delete(sess.Values, "team_invite_uuid")
				sess.Save(c.Request(), c.Response())
			}

			var isAdmin = false
			// If no team invitation, we need to create a new team
			if assignedTeamID == nil {
				isAdmin = true
				// Provider-specific handling to get team name
				switch providerName {
				case "slack":
					c.Logger().Infof("Received Slack auth request")
					// Get the team name from Slack
					resp, err := getTeamInfoRawJSON(user.AccessToken)
					if err != nil {
						return fmt.Errorf("failed to get team info: %w", err)
					}
					name := gjson.Get(string(resp), "team.name")
					if name.Exists() {
						teamName = name.String()
					}
				case "google":
					c.Logger().Infof("Received Google auth request")
				case "github":
					c.Logger().Infof("Received GitHub auth request")
					// Get the company from GitHub user data
					if user.RawData != nil {
						rawData, err := json.Marshal(user.RawData)
						if err != nil {
							c.Logger().Warnf("Failed to marshal GitHub RawData: %v", err)
						} else {
							company := gjson.Get(string(rawData), "company")
							if company.Exists() && company.String() != "" {
								// Remove @ symbol if present
								companyStr := strings.TrimPrefix(company.String(), "@")
								teamName = companyStr + "-Team"
							}
						}
					} else {
						c.Logger().Warn("GitHub RawData is nil")
					}
				}

				// Use fallback team name if none provided
				if teamName == "" {
					teamName = fmt.Sprintf("%s-Team", user.FirstName)
				}

				// Create a new team
				team := models.Team{
					Name: teamName,
				}
				if err := tx.Create(&team).Error; err != nil {
					return fmt.Errorf("failed to create team: %w", err)
				}
				assignedTeamID = &team.ID
			}

			u = models.User{
				FirstName: user.FirstName,
				LastName:  user.LastName,
				Email:     user.Email,
				AvatarURL: user.AvatarURL,
				TeamID:    assignedTeamID,
				IsAdmin:   isAdmin,
			}
			if err := tx.Create(&u).Error; err != nil {
				return fmt.Errorf("failed to create user: %w", err)
			}

			switch providerName {
			case "slack":
				// Update to higher resolution image
				rawData, _ := json.Marshal(user.RawData)
				avatar := gjson.Get(string(rawData), "user.profile.image_512")
				if avatar.Exists() {
					u.AvatarURL = avatar.String()
				}

				// Get the team members
				resp, err := getTeamMembersRawJSON(user.AccessToken)
				if err != nil {
					return fmt.Errorf("failed to get team members: %w", err)
				}

				var result map[string]interface{}
				if err := json.Unmarshal([]byte(resp), &result); err != nil {
					return fmt.Errorf("failed to parse team members: %w", err)
				}
				u.SocialMetadata = result
				if err := tx.Save(&u).Error; err != nil {
					return fmt.Errorf("failed to update user: %w", err)
				}
			case "github":
				// Store GitHub user data in SocialMetadata
				if user.RawData != nil {
					rawData, err := json.Marshal(user.RawData)
					if err != nil {
						c.Logger().Warnf("Failed to marshal GitHub RawData for metadata: %v", err)
					} else {
						var result map[string]interface{}
						if err := json.Unmarshal(rawData, &result); err != nil {
							c.Logger().Warnf("Failed to parse GitHub user data: %v", err)
						} else {
							u.SocialMetadata = result

							// If FirstName is empty, use name or login from GitHub raw data
							if u.FirstName == "" {
								if name := gjson.Get(string(rawData), "name"); name.Exists() && name.String() != "" {
									u.FirstName = name.String()
								} else if login := gjson.Get(string(rawData), "login"); login.Exists() {
									u.FirstName = login.String()
								}
							}

							if err := tx.Save(&u).Error; err != nil {
								c.Logger().Errorf("Failed to save GitHub metadata: %v", err)
								return fmt.Errorf("failed to update user: %w", err)
							}
						}
					}
				} else {
					c.Logger().Warn("GitHub RawData is nil, skipping metadata storage")
				}
			}
		} else {
			// User already exists, check if they have a team invite UUID in session
			// This handles the case where an existing user clicks an invite link and logs in via social auth
			sess, err := session.Get("session", c)
			if err == nil {
				inviteUUID := sess.Values["team_invite_uuid"]
				if inviteUUID != nil {
					var invitation models.TeamInvitation
					if err := tx.Where("unique_id = ?", inviteUUID).Preload("Team").First(&invitation).Error; err == nil {
						// Check if user is already in this team
						if u.TeamID == nil || int(*u.TeamID) != invitation.TeamID {
							// Check if user has teammates (similar to ChangeTeam logic)
							teammates, err := u.GetTeammates(tx)
							if err != nil {
								return fmt.Errorf("failed to get user teammates: %w", err)
							}

							teammateCount := len(teammates)
							if teammateCount > 0 {
								message := fmt.Sprintf("🚨 User %s attempted to change teams but has %d teammate(s). Invitation UUID: %s",
									u.ID,
									teammateCount,
									inviteUUID)
								c.Logger().Warnf("User %s attempted to change teams via social auth but has %d teammate(s). Invitation UUID: %s",
									u.ID, teammateCount, inviteUUID)
								_ = notifications.SendTelegramNotification(message, h.Config)
							} else {
								teamID := uint(invitation.TeamID)
								u.TeamID = &teamID
								u.Team = &invitation.Team
								u.IsAdmin = false
								if err := tx.Save(&u).Error; err != nil {
									return fmt.Errorf("failed to update user team: %w", err)
								}
								c.Logger().Infof("Changed user %s team to %d via social auth with invite", u.ID, invitation.TeamID)
							}
						}
					}
					// Clean up the session
					delete(sess.Values, "team_invite_uuid")
					sess.Save(c.Request(), c.Response())
				}
			}
		}

		return nil
	})

	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, err.Error())
	}

	// Send welcome email if a new user was created
	if isNewUser && h.EmailClient != nil {
		h.EmailClient.SendWelcomeEmail(&u)
	}

	// Create a JWT token
	token, err := h.JwtIssuer.GenerateToken(u.Email)
	if err != nil {
		return c.String(http.StatusInternalServerError, "Failed to generate token")
	}

	_ = notifications.SendTelegramNotification(fmt.Sprintf("New sign-in: %s", u.ID), h.Config)

	// Redirect to the web app with the JWT token
	return c.Redirect(http.StatusFound, fmt.Sprintf("/login?token=%s", token))
}

func (h *AuthHandler) SocialLogin(c echo.Context) error {
	provider := c.Param("provider")

	// In case users were invited to join a team, we'll pass the invite UUID
	// to the callback
	inviteUUID := c.QueryParam("invite_uuid")
	if inviteUUID != "" {
		sess, err := session.Get("session", c)
		if err == nil {
			sess.Values["team_invite_uuid"] = inviteUUID
			err := sess.Save(c.Request(), c.Response())
			if err != nil {
				c.Logger().Errorf("Failed to save team invite in social login session: %v", err)
			}
		}
	}

	req := c.Request()
	// Set the provider in the query parameters for gothic to work
	q := req.URL.Query()
	q.Set("provider", provider)
	req.URL.RawQuery = q.Encode()

	gothic.BeginAuthHandler(c.Response(), req)
	return nil
}

func (h *AuthHandler) ManualSignUp(c echo.Context) error {
	c.Logger().Info("Received manual sign-up request")

	type SignUpRequest struct {
		models.User
		TeamName       string `json:"team_name"`
		TeamInviteUUID string `json:"team_invite_uuid"`
	}

	req := new(SignUpRequest)
	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	u := &req.User
	if err := c.Validate(u); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	if burner.IsBurnerEmail(u.Email) {
		return echo.NewHTTPError(http.StatusBadRequest, "Temporary email addresses are not allowed")
	}

	// Check if team invite UUID was provided
	if req.TeamInviteUUID != "" {
		// Find the team invitation
		var invitation models.TeamInvitation
		result := h.DB.Where("unique_id = ?", req.TeamInviteUUID).First(&invitation)
		if result.Error == nil {
			// Set the user's team ID
			teamID := uint(invitation.TeamID)
			u.TeamID = &teamID
		}
	}

	if req.TeamName != "" {
		// Create a new team
		team := models.Team{
			Name: req.TeamName,
		}
		h.DB.Create(&team)
		u.TeamID = &team.ID
		u.IsAdmin = true
	}

	result := h.DB.Create(u)
	if errors.Is(result.Error, gorm.ErrDuplicatedKey) {
		return echo.NewHTTPError(409, "user with this email already exists")
	}

	// Handle other potential errors during creation
	if result.Error != nil {
		c.Logger().Errorf("Failed to create user: %v", result.Error)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create user")
	}

	// Send welcome email after successful creation
	if h.EmailClient != nil {
		h.EmailClient.SendWelcomeEmail(u)
	}

	// Create a JWT token
	token, err := h.JwtIssuer.GenerateToken(u.Email)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to generate token")
	}

	_ = notifications.SendTelegramNotification(fmt.Sprintf("New sign-up: %s", u.ID), h.Config)

	return c.JSON(http.StatusCreated, map[string]string{"token": token})
}

func (h *AuthHandler) ManualSignIn(c echo.Context) error {
	c.Logger().Info("Received manual sign-in request")
	req := &SignInRequest{}

	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	if err := c.Validate(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	u := &models.User{}
	result := h.DB.Where("email = ?", req.Email).First(u)
	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return echo.NewHTTPError(http.StatusUnauthorized, "Invalid email or password")
	}

	if !u.CheckPassword(req.Password) {
		return echo.NewHTTPError(http.StatusUnauthorized, "Invalid email or password")
	}

	// Create a JWT token
	token, err := h.JwtIssuer.GenerateToken(u.Email)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to generate token")
	}

	_ = notifications.SendTelegramNotification(fmt.Sprintf("New sign-in: %s", u.ID), h.Config)

	return c.JSON(http.StatusOK, map[string]string{"token": token})
}

func (h *AuthHandler) ForgotPassword(c echo.Context) error {
	const verificationMessage = "If the email you specified exists in our system, we've sent a password reset link to it."
	c.Logger().Info("Received forgot password request")
	req := &ForgotPasswordRequest{}
	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}
	if err := c.Validate(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	// Check if the user exists
	u := &models.User{}
	user := h.DB.Where("email = ?", req.Email).First(u)
	// Always return success message to avoid user enumeration
	// https://ux.stackexchange.com/questions/87079/reset-password-appropriate-response-if-email-doesnt-exist/87093#87093
	if errors.Is(user.Error, gorm.ErrRecordNotFound) {
		return c.JSON(http.StatusOK, map[string]string{"message": verificationMessage})
	}

	// Check if the token for the user exists and is still valid
	resetPasswordToken := &models.ResetToken{}
	token := h.DB.Where("user_id = ?", u.ID).
		Order("created_at DESC").First(resetPasswordToken)

	// Create a new token if none exists or if the existing one is invalid/used
	if errors.Is(token.Error, gorm.ErrRecordNotFound) || !resetPasswordToken.IsValid() || resetPasswordToken.Used() {
		resetToken := &models.ResetToken{UserID: u.ID}
		if err := resetToken.CreateResetToken(h.DB); err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create password reset token")
		}
		resetPasswordToken = resetToken

	}

	baseURL := "https://" + h.Config.Server.DeployDomain
	if h.EmailClient != nil {
		resetLink := fmt.Sprintf("%s/reset-password/%s", baseURL, resetPasswordToken.Token)
		h.EmailClient.SendPasswordResetEmail(u.Email, resetLink)
	}
	return c.JSON(http.StatusOK, map[string]string{"message": verificationMessage})
}

func (h *AuthHandler) ResetPassword(c echo.Context) error {
	c.Logger().Info("Received reset password request")
	req := &ResetPasswordRequest{}
	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}
	if err := c.Validate(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}
	tokenString := c.Param("token")
	if tokenString == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Missing token")
	}

	// Check if the token for the user exists and is still valid
	resetPasswordToken := &models.ResetToken{}
	token := h.DB.Where("token = ?", tokenString).
		Order("created_at DESC").First(resetPasswordToken)
	if errors.Is(token.Error, gorm.ErrRecordNotFound) || !resetPasswordToken.IsValid() {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid or expired token")
	}
	if resetPasswordToken.Used() {
		return echo.NewHTTPError(http.StatusBadRequest, "This password reset link has already been used")
	}

	// Find the user by user ID from the token
	u := &models.User{}
	result := h.DB.Where("id = ?", resetPasswordToken.UserID).First(u)
	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return echo.NewHTTPError(http.StatusNotFound, "User not found")
	}
	// Reset the user's password
	hashedPassword, err := models.HashPassword(req.Password)
	if err != nil {
		c.Logger().Error("Failed to hash password:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to reset password")
	}
	u.HashedPassword = hashedPassword
	u.Password = ""
	if err := h.DB.Save(u).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to reset password")
	}

	// Mark the password reset token as used
	if err := h.DB.Where("token = ?", tokenString).First(&resetPasswordToken).Error; err == nil {
		now := time.Now()
		resetPasswordToken.UsedAt = &now
		if err := h.DB.Save(&resetPasswordToken).Error; err != nil {
			c.Logger().Warn("Failed to mark password reset token as used:", err)
		}
	}
	return c.JSON(http.StatusOK, map[string]string{"message": "Your password has been changed. You can now use it to log in."})
}

func (h *AuthHandler) UserPage(c echo.Context) error {

	sess, err := session.Get("session", c)
	if err != nil {
		return c.String(http.StatusInternalServerError, "Failed to get session")
	}

	// Check if the user came from the app
	redirectToApp, ok := sess.Values["redirect_to_app"].(bool)
	shouldRedirect := ok && redirectToApp

	// If we need to redirect, clean up the session first
	if shouldRedirect {
		delete(sess.Values, "redirect_to_app")
		if err := sess.Save(c.Request(), c.Response()); err != nil {
			return c.String(http.StatusInternalServerError, "Failed to save session")
		}
	}

	user := &models.User{}
	h.DB.Where("email = ?", sess.Values["email"].(string)).First(user)

	// Pass the redirect flag to the template
	data := map[string]interface{}{
		"User":           user,
		"ShouldRedirect": shouldRedirect,
	}

	err = c.Render(http.StatusOK, "user.html", data)
	if err != nil {
		c.Logger().Error(err)
	}

	return err
}

// AuthenticateApp is an endpoint that will be create a
// JWT token to be used by the app
func (h *AuthHandler) AuthenticateApp(c echo.Context) error {

	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)

	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	// Create a JWT token
	token, err := h.JwtIssuer.GenerateToken(user.Email)
	if err != nil {
		return c.String(http.StatusInternalServerError, "Failed to generate token")
	}

	return c.JSON(http.StatusOK, map[string]string{"token": token})
}

func (h *AuthHandler) User(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized here")
	}

	// We need additional payload for subscription information
	userWithSubscription, err := models.GetUserWithSubscription(h.DB, user)
	if err != nil {
		return c.JSON(http.StatusInternalServerError, map[string]string{"error": err.Error()})
	}

	return c.JSON(http.StatusOK, userWithSubscription)
}

func (h *AuthHandler) Teammates(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	teammates, err := user.GetTeammates(h.DB)
	if err != nil {
		return c.JSON(http.StatusInternalServerError, map[string]string{"error": err.Error()})
	}

	// Check Redis for active users
	ctx := context.Background()
	for i := range teammates {
		// Check if user has an active Redis subscription
		channelPattern := common.GetUserChannel(teammates[i].ID)
		channels, err := h.Redis.PubSubChannels(ctx, channelPattern).Result()
		if err != nil {
			c.Logger().Error("Error checking Redis channels:", err)
			continue
		}
		teammates[i].IsActive = len(channels) > 0
	}

	return c.JSON(http.StatusOK, teammates)
}

func (h *AuthHandler) GenerateDebugCallToken(c echo.Context) error {
	email := c.QueryParam("email")
	// Find user by email
	var user models.User
	result := h.ServerState.DB.Where("email = ?", email).First(&user)

	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return c.String(http.StatusNotFound, "User not found")
	}
	tokens, err := generateLiveKitTokens(&h.ServerState, "random-name-for-now", &user)
	if err != nil {
		return c.String(http.StatusInternalServerError, "Failed to generate callee tokens")
	}

	tokens.Participant = user.ID

	return c.JSON(http.StatusOK, tokens)
}

func (h *AuthHandler) UpdateName(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized")
	}

	type UpdateRequest struct {
		FirstName string `json:"first_name"`
		LastName  string `json:"last_name"`
	}

	req := new(UpdateRequest)
	if err := c.Bind(req); err != nil {
		c.Logger().Error("Failed to bind request:", err)
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	user.FirstName = req.FirstName
	user.LastName = req.LastName

	if err := h.DB.Save(user).Error; err != nil {
		c.Logger().Error("Failed to save to db:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update user")
	}

	return c.JSON(http.StatusOK, user)
}

// GetInviteUUID generates or returns an existing team invitation UUID for the authenticated user's team
func (h *AuthHandler) GetInviteUUID(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Unauthorized")
	}

	// Check if user has a team
	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User is not part of any team")
	}

	teamID := int(*user.TeamID)

	// Check if there's an existing invitation for this team
	var invitation models.TeamInvitation
	result := h.DB.Where("team_id = ?", teamID).First(&invitation)

	// Create a new invitation if none exists or if previous one was expired
	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		// Generate a UUID for the invitation
		inviteUUID, err := uuid.NewV7()
		if err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to generate invitation UUID")
		}

		invitation = models.TeamInvitation{
			TeamID:   teamID,
			UniqueID: inviteUUID.String(),
		}

		if err := h.DB.Create(&invitation).Error; err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create team invitation")
		}
	}

	// Get team name (only query for what we need)
	var team models.Team
	if err := h.DB.Select("name").Where("id = ?", teamID).First(&team).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team information")
	}

	return c.JSON(http.StatusOK, map[string]string{
		"invite_uuid": invitation.UniqueID,
		"team_name":   team.Name,
	})
}

// GetInvitationDetails retrieves the team details for a given invitation UUID
func (h *AuthHandler) GetInvitationDetails(c echo.Context) error {
	uuid := c.Param("uuid")
	if uuid == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid invitation UUID")
	}

	// Find the team invitation by UUID
	var invitation models.TeamInvitation
	result := h.DB.Where("unique_id = ?", uuid).Preload("Team").First(&invitation)
	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return echo.NewHTTPError(http.StatusNotFound, "Invitation not found or has expired")
		}
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to retrieve invitation details")
	}

	// Return team information with the invitation UUID for sign up
	return c.JSON(http.StatusOK, invitation.Team)
}

// SendTeamInvites sends invitation emails to join a team
func (h *AuthHandler) SendTeamInvites(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Unauthorized")
	}

	// Check if user has a team
	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User is not part of any team")
	}

	teamID := int(*user.TeamID)

	// Get the team name
	var team models.Team
	if err := h.DB.Select("name").Where("id = ?", teamID).First(&team).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team information")
	}

	// Parse request body
	type InviteRequest struct {
		Invitees []string `json:"invitees" validate:"required,dive,email"`
	}

	req := new(InviteRequest)
	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid request format")
	}

	if err := c.Validate(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid email addresses")
	}

	// Ensure we have a valid team invitation UUID
	var invitation models.TeamInvitation
	result := h.DB.Where("team_id = ?", teamID).First(&invitation)

	// Create a new invitation if none exists
	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		// Generate a UUID for the invitation
		inviteUUID, err := uuid.NewV7()
		if err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to generate invitation UUID")
		}

		invitation = models.TeamInvitation{
			TeamID:   teamID,
			UniqueID: inviteUUID.String(),
		}

		if err := h.DB.Create(&invitation).Error; err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create team invitation")
		}
	}

	// Process invitations in a goroutine to not block the response
	baseURL := "https://" + h.Config.Server.DeployDomain
	inviteLink := fmt.Sprintf("%s/invitation/%s", baseURL, invitation.UniqueID)
	inviterName := user.FirstName + " " + user.LastName

	// Limit also the user to 50 invites per day
	// just to avoid abuse of our service
	var invitesToday int64
	h.DB.Model(&models.EmailInvitation{}).Where("sent_by = ? AND sent_at > ?", user.ID, time.Now().AddDate(0, 0, -1)).Count(&invitesToday)

	c.Echo().Logger.Infof("Invites today by user %s: %d", user.ID, invitesToday)

	if invitesToday >= 50 {
		return echo.NewHTTPError(http.StatusTooManyRequests, "You have reached the maximum number of invites per day")
	}

	for idx, email := range req.Invitees {
		if (idx + int(invitesToday)) >= 50 {
			c.Echo().Logger.Info("Skipping inviting more emails because of rate limit for user:", user.ID)
			break
		}
		// Check if we can send an invitation to this email (rate limit check)
		if !models.CanSendInvite(h.DB, email) {
			// Skip this email silently
			c.Echo().Logger.Info("Skipping inviting email:", email)
			continue
		}

		// Record the invitation in the database
		emailInvite := models.EmailInvitation{
			TeamID: teamID,
			Email:  email,
			SentAt: time.Now(),
			SentBy: user.ID,
		}
		h.DB.Create(&emailInvite)

		// Send the email if email client is available
		if h.EmailClient != nil {
			h.EmailClient.SendTeamInvitationEmail(inviterName, team.Name, inviteLink, email)
		}
	}

	return c.NoContent(http.StatusOK)
}

// UpdateOnboardingFormStatus updates the user's metadata to mark the onboarding form as completed
func (h *AuthHandler) UpdateOnboardingFormStatus(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Unauthorized")
	}

	type OnboardingRequest struct {
		Onboarding map[string]interface{} `json:"onboarding"`
	}

	req := new(OnboardingRequest)
	if err := c.Bind(req); err != nil {
		c.Logger().Error("Failed to bind request:", err)
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	// Initialize metadata if it doesn't exist
	if user.Metadata == nil {
		user.Metadata = make(map[string]interface{})
	}

	// Set the onboarding form data
	user.Metadata["hasFilledOnboardingForm"] = true
	if req.Onboarding != nil {
		user.Metadata["onboarding"] = req.Onboarding
	}

	// Save the updated user
	if err := h.DB.Save(user).Error; err != nil {
		c.Logger().Error("Failed to update user metadata:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update onboarding status")
	}

	return c.NoContent(http.StatusOK)
}

// Get all rooms for the user
func (h *AuthHandler) GetRooms(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	var rooms []models.Room
	// First, check if the room exists
	result := h.DB.Where("team_id = ?", user.TeamID).Find(&rooms)

	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return c.String(http.StatusNotFound, "Rooms not found")
	}

	return c.JSON(http.StatusOK, rooms)
}

// CreateRoom creates a new room for the user.
func (h *AuthHandler) CreateRoom(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	type Room struct {
		Name string `gorm:"not null" json:"name" validate:"required"`
	}

	req := &Room{}

	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	room := models.Room{
		Name:   req.Name,
		UserID: user.ID,
		Team:   user.Team,
		TeamID: user.TeamID,
	}

	if err := h.DB.Create(&room).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create room")
	}

	// Send Telegram notification for room creation
	_ = notifications.SendTelegramNotification(fmt.Sprintf("Room created: '%s' by user %s", room.Name, user.ID), h.Config)

	return c.JSON(http.StatusOK, room)
}

// UpdateRoom updates an existing room for the user.
func (h *AuthHandler) UpdateRoom(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	roomID := c.Param("id")

	type Room struct {
		Name string `gorm:"not null" json:"name" validate:"required"`
	}

	req := &Room{}

	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	var room models.Room

	result := h.DB.Where("id = ?", roomID).First(&room)

	// Check if user can modify the room
	if user.Team != room.Team {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return c.String(http.StatusNotFound, "Room not found")
	}
	room.Name = req.Name

	if err := h.DB.Save(&room).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create room")
	}

	// Send Telegram notification for room modification
	_ = notifications.SendTelegramNotification(fmt.Sprintf("Room modified: '%s' by user %s", room.Name, user.ID), h.Config)

	return c.JSON(http.StatusOK, room)
}

func (h *AuthHandler) DeleteRoom(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	roomID := c.Param("id")

	var room models.Room

	// First, check if the room exists
	result := h.DB.Where("id = ?", roomID).First(&room)

	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return c.String(http.StatusNotFound, "Room not found")
	}

	// Check if user can modify the room
	if user.Team != room.Team {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	// Delete the room
	if err := h.DB.Delete(&room).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to delete room")
	}

	return c.NoContent(http.StatusNoContent)
}

func (h *AuthHandler) GetRoom(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	roomID := c.Param("id")
	var room models.Room

	// First, check if the room exists
	result := h.DB.Where("id = ?", roomID).First(&room)

	if errors.Is(result.Error, gorm.ErrRecordNotFound) {
		return c.String(http.StatusNotFound, "Room not found")
	}

	// Check if user can access the room
	if user.Team != room.Team {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	tokens, err := generateLiveKitTokens(&h.ServerState, room.ID, user)
	if err != nil {
		c.Logger().Error("Failed to generate room tokens:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to generate tokens")
	}
	tokens.Participant = user.ID

	_ = notifications.SendTelegramNotification(fmt.Sprintf("User %s joined the %s room", user.ID, room.Name), h.Config)

	return c.JSON(http.StatusOK, tokens)
}

func (h *AuthHandler) GetLivekitServerURL(c echo.Context) error {
	_, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	return c.JSON(http.StatusOK, map[string]string{
		"url": h.Config.Livekit.ServerURL,
	})
}

// SubscribeToLinuxWaitingList subscribes the user to the Linux waiting list
// and unsubscribes from marketing emails
func (h *AuthHandler) SubscribeToLinuxWaitingList(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	user.EmailSubscriptions.LinuxWaitingList = true
	user.EmailSubscriptions.MarketingEmails = false

	if err := h.DB.Save(user).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update user preferences")
	}

	return c.JSON(http.StatusOK, map[string]string{
		"message": "Successfully subscribed to Linux waiting list",
	})
}

// ChangeTeam allows a logged-in user to change teams using an invitation UUID.
// It validates the user has no teammates before allowing the change
func (h *AuthHandler) ChangeTeam(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	invitationUUID := c.Param("uuid")

	var invitation models.TeamInvitation
	result := h.DB.Where("unique_id = ?", invitationUUID).Preload("Team").First(&invitation)
	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return echo.NewHTTPError(http.StatusNotFound, "Invitation not found or has expired")
		}
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to retrieve invitation details")
	}

	if invitation.TeamID == int(*user.TeamID) {
		return c.NoContent(http.StatusNoContent)
	}

	teammates, err := user.GetTeammates(h.DB)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get user teammates")
	}

	teammateCount := len(teammates)

	if teammateCount > 0 {
		// Send telegram notification for attention
		message := fmt.Sprintf("🚨 User %s attempted to change teams but has %d teammate(s). Invitation UUID: %s",
			user.ID,
			teammateCount,
			invitationUUID)

		_ = notifications.SendTelegramNotification(message, h.Config)

		return echo.NewHTTPError(http.StatusConflict, fmt.Sprintf("Cannot change teams: you currently have %d teammate(s). Please contact support for assistance.", teammateCount))
	}

	c.Logger().Infof("Changing user %s team to %d", user.ID, invitation.TeamID)

	teamID := uint(invitation.TeamID)
	user.TeamID = &teamID
	user.Team = &invitation.Team
	user.IsAdmin = false

	if err := h.DB.Save(&user).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update user team")
	}

	return c.JSON(http.StatusOK, map[string]interface{}{
		"message":   "Successfully changed team",
		"team_name": invitation.Team.Name,
		"team_id":   invitation.TeamID,
	})
}

// RemoveTeammate removes a user from a team and creates a new solo team for them
// removed user will also receive an email notification
func (h *AuthHandler) RemoveTeammate(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized request")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User is not part of any team")
	}

	// Preload team to avoid extra query for email
	if err := h.DB.Preload("Team").Where("id = ?", user.ID).First(user).Error; err != nil {
		c.Logger().Error("Failed to load user team:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "failed to load user")
	}

	if user.Team == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User team not found")
	}

	teammateID := c.Param("userId")
	if teammateID == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "userId is required")
	}

	if user.ID == teammateID {
		return echo.NewHTTPError(http.StatusBadRequest, "cannot remove yourself")
	}

	if !user.IsAdmin {
		return echo.NewHTTPError(http.StatusForbidden, "admin required")
	}

	var teammate models.User
	if err := h.DB.Select("id, team_id, is_admin, first_name, last_name, email").Where("id = ?", teammateID).First(&teammate).Error; err != nil {
		if errors.Is(err, gorm.ErrRecordNotFound) {
			return echo.NewHTTPError(http.StatusNotFound, "user not found")
		}
		return echo.NewHTTPError(http.StatusInternalServerError, "failed to load user")
	}

	if teammate.TeamID == nil || *teammate.TeamID != *user.TeamID {
		return echo.NewHTTPError(http.StatusForbidden, "user not in your team")
	}

	oldTeamName := user.Team.Name
	var newTeamName string

	if err := h.DB.Transaction(func(tx *gorm.DB) error {

		// Create new team
		newTeamName := fmt.Sprintf("team-%s", uuid.NewString()[:8])
		newTeam := models.Team{
			Name: newTeamName,
		}
		if err := tx.Create(&newTeam).Error; err != nil {
			return err
		}

		// assign new team to removed user
		if err := tx.Model(&models.User{}).
			Where("id = ?", teammate.ID).
			Updates(map[string]any{
				"team_id":  newTeam.ID,
				"is_admin": true,
			}).Error; err != nil {
			return err
		}

		// Update subscription quantity if there is a subscription for the old team
		if err := models.UpdateSubscriptionQuantity(tx, *user.TeamID); err != nil {
			return err
		}

		return nil
	}); err != nil {
		c.Logger().Error("RemoveTeammate error:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "failed to remove teammate")
	}

	// Send email to removed user
	if h.EmailClient != nil {
		h.EmailClient.SendTeamRemovalEmail(&teammate, oldTeamName, newTeamName)
	}

	return c.NoContent(http.StatusNoContent)
}

// UnsubscribeUser handles both GET and POST requests for unsubscribing users.
// Follows instructions from:
// https://resend.com/docs/dashboard/emails/add-unsubscribe-to-transactional-emails
func (h *AuthHandler) UnsubscribeUser(c echo.Context) error {
	token := c.Param("token")
	if token == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Token is required")
	}

	// Find user by "unsubscribe" token
	var user models.User
	result := h.DB.Where("unsubscribe_id = ?", token).First(&user)
	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return echo.NewHTTPError(http.StatusNotFound, "User not found")
		}
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to retrieve user details, cannot unsubscribe")
	}

	// Handle POST request (one-click unsubscribe)
	if c.Request().Method == http.MethodPost {
		// Unsubscribe user from all emails
		if err := user.UnsubscribeFromAllEmails(h.DB); err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to unsubscribe")
		}

		return c.String(http.StatusOK, "You are now unsubscribed from all marketing emails 🥲")
	}

	// Handle GET request (show unsubscribe page)
	if c.Request().Method == http.MethodGet {
		// Check if already unsubscribed
		if user.EmailSubscriptions.UnsubscribedAt != nil {
			return c.Render(http.StatusOK, "unsubscribe-success.html", nil)
		}

		// Show unsubscribe form
		data := map[string]interface{}{
			"Email": user.Email,
			"Token": token,
		}
		return c.Render(http.StatusOK, "unsubscribe-form.html", data)
	}

	return echo.NewHTTPError(http.StatusMethodNotAllowed, "Method not allowed")
}

// SubmitFeedback handles post-call feedback submissions
func (h *AuthHandler) SubmitFeedback(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return echo.NewHTTPError(http.StatusUnauthorized, "Unauthorized")
	}

	feedback := new(models.Feedback)
	if err := c.Bind(feedback); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	if err := c.Validate(feedback); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	// Set the participant ID from the authenticated user
	feedback.ParticipantID = user.ID

	if err := h.DB.Create(feedback).Error; err != nil {
		c.Logger().Error("Failed to create feedback:", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to save feedback")
	}

	// Send Telegram notification
	var message string
	scoreEmojis := []string{"😡", "😕", "😐", "😊", "🤩"}
	scoreEmoji := scoreEmojis[feedback.Score-1]

	if feedback.Feedback != "" {
		message = fmt.Sprintf("📝 Call Feedback\nUser: %s\nScore: %d/5 %s\nFeedback: %s",
			user.ID, feedback.Score, scoreEmoji, feedback.Feedback)
	} else {
		message = fmt.Sprintf("📝 Call Feedback\nUser: %s\nScore: %d/5 %s",
			user.ID, feedback.Score, scoreEmoji)
	}

	_ = notifications.SendTelegramNotification(message, h.Config)

	return c.NoContent(http.StatusCreated)
}
