package handlers

import (
	"errors"
	"net/http"

	"hopp-backend/internal/models"

	"github.com/labstack/echo/v4"
	"gorm.io/gorm"
)

const (
	signupDisabledMessage        = "Sign-up is disabled on this instance. Ask a team admin for an invitation link."
	passwordLoginDisabledMessage = "Email and password login is disabled on this instance."
)

// errSignupDisabled is returned from inside the sign-up transactions when an
// account without a team invitation is rejected because of DISABLE_SIGNUP.
var errSignupDisabled = errors.New("sign-up is disabled")

// InstanceConfigResponse is the public, unauthenticated description of how this
// instance is configured. Clients use it to hide flows the backend would reject.
type InstanceConfigResponse struct {
	SignupEnabled        bool     `json:"signup_enabled"`
	PasswordLoginEnabled bool     `json:"password_login_enabled"`
	BillingEnabled       bool     `json:"billing_enabled"`
	AuthProviders        []string `json:"auth_providers"`
}

// GetInstanceConfig returns the instance configuration relevant to clients.
func (h *AuthHandler) GetInstanceConfig(c echo.Context) error {
	// Reported as enabled while the instance is empty, so the first account
	// can be created from the web app even with DISABLE_SIGNUP.
	signupEnabled, err := h.uninvitedSignupAllowed(h.DB)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to load instance configuration")
	}

	return c.JSON(http.StatusOK, InstanceConfigResponse{
		SignupEnabled:        signupEnabled,
		PasswordLoginEnabled: !h.Config.Auth.DisablePasswordLogin,
		BillingEnabled:       h.Config.IsStripeEnabled(),
		AuthProviders:        h.Config.SocialProviders(),
	})
}

// RequirePasswordLogin rejects email/password endpoints when
// DISABLE_PASSWORD_LOGIN is set.
func (h *AuthHandler) RequirePasswordLogin(next echo.HandlerFunc) echo.HandlerFunc {
	return func(c echo.Context) error {
		if h.Config.Auth.DisablePasswordLogin {
			return echo.NewHTTPError(http.StatusForbidden, passwordLoginDisabledMessage)
		}
		return next(c)
	}
}

// uninvitedSignupAllowed reports whether an account may be created without a
// team invitation. With DISABLE_SIGNUP this is only the case while the instance
// has no users at all, so a fresh deployment can still be bootstrapped.
func (h *AuthHandler) uninvitedSignupAllowed(tx *gorm.DB) (bool, error) {
	if !h.Config.Auth.DisableSignup {
		return true, nil
	}

	var count int64
	// Unscoped: a soft-deleted user still means the instance was bootstrapped.
	if err := tx.Unscoped().Model(&models.User{}).Count(&count).Error; err != nil {
		return false, err
	}
	return count == 0, nil
}
