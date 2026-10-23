package handlers

import (
	"net/http"

	"github.com/labstack/echo/v4"
	"gorm.io/gorm"

	"hopp-backend/internal/models"
)

const errInviteRequiredMessage = "Sign-up is by invitation only. Ask a teammate for an invite link."

// errInviteRequired is returned when an account would be created without a
// valid team invite on an instance that requires one.
func errInviteRequired() error {
	return echo.NewHTTPError(http.StatusForbidden, errInviteRequiredMessage)
}

// inviteRequiredForNewTeam reports whether creating an account that is not
// joining an existing team must be refused. The first account on an empty
// instance is always allowed so a fresh install can be bootstrapped.
func inviteRequiredForNewTeam(db *gorm.DB) (bool, error) {
	settings, err := models.GetInstanceSettings(db)
	if err != nil {
		return false, err
	}
	if !settings.SignupRequiresInvite {
		return false, nil
	}
	var users int64
	if err := db.Model(&models.User{}).Count(&users).Error; err != nil {
		return false, err
	}
	return users > 0, nil
}

type instanceSettingsResponse struct {
	// IsOwner is true when the caller may change the settings.
	IsOwner              bool `json:"is_owner"`
	SignupRequiresInvite bool `json:"signup_requires_invite"`
}

func (h *AuthHandler) instanceSettingsFor(c echo.Context, userID string) (*instanceSettingsResponse, error) {
	ownerID, err := models.GetInstanceOwnerID(h.DB)
	if err != nil {
		c.Logger().Error("Failed to get instance owner: ", err)
		return nil, echo.NewHTTPError(http.StatusInternalServerError, "Failed to get instance settings")
	}
	settings, err := models.GetInstanceSettings(h.DB)
	if err != nil {
		c.Logger().Error("Failed to get instance settings: ", err)
		return nil, echo.NewHTTPError(http.StatusInternalServerError, "Failed to get instance settings")
	}
	return &instanceSettingsResponse{
		IsOwner:              ownerID != "" && ownerID == userID,
		SignupRequiresInvite: settings.SignupRequiresInvite,
	}, nil
}

// GetInstanceSettings returns the server-wide settings and whether the caller
// is the instance owner (the only user who may change them).
func (h *AuthHandler) GetInstanceSettings(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized")
	}

	resp, err := h.instanceSettingsFor(c, user.ID)
	if err != nil {
		return err
	}
	return c.JSON(http.StatusOK, resp)
}

// UpdateInstanceSettings changes the server-wide settings. Only the instance
// owner may call it: is_admin is per-team, so on an instance with open sign-up
// every stranger is an admin of their own team and must not be able to
// reopen sign-up.
func (h *AuthHandler) UpdateInstanceSettings(c echo.Context) error {
	user, isAuthenticated := h.getAuthenticatedUserFromJWT(c)
	if !isAuthenticated {
		return c.String(http.StatusUnauthorized, "Unauthorized")
	}

	type UpdateInstanceSettingsRequest struct {
		SignupRequiresInvite *bool `json:"signup_requires_invite" validate:"required"`
	}
	req := new(UpdateInstanceSettingsRequest)
	if err := c.Bind(req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}
	if req.SignupRequiresInvite == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "signup_requires_invite is required")
	}

	ownerID, err := models.GetInstanceOwnerID(h.DB)
	if err != nil {
		c.Logger().Error("Failed to get instance owner: ", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update instance settings")
	}
	if ownerID == "" || ownerID != user.ID {
		return echo.NewHTTPError(http.StatusForbidden, "Only the instance owner can change instance settings")
	}

	if _, err := models.SetSignupRequiresInvite(h.DB, *req.SignupRequiresInvite); err != nil {
		c.Logger().Error("Failed to update instance settings: ", err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update instance settings")
	}

	resp, err := h.instanceSettingsFor(c, user.ID)
	if err != nil {
		return err
	}
	return c.JSON(http.StatusOK, resp)
}
