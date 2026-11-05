package handlers

import (
	"net/http"

	"github.com/labstack/echo/v4"
	"github.com/markbates/goth"
)

// OIDCConfigResponse describes the generic OpenID Connect login.
type OIDCConfigResponse struct {
	Enabled     bool   `json:"enabled"`
	DisplayName string `json:"display_name"`
}

// InstanceConfigResponse is the public, unauthenticated description of how this
// instance is configured. Clients use it to decide which flows to offer.
type InstanceConfigResponse struct {
	OIDC OIDCConfigResponse `json:"oidc"`
}

// GetInstanceConfig returns the instance configuration relevant to clients.
func (h *AuthHandler) GetInstanceConfig(c echo.Context) error {
	// Enabled only if the provider was actually registered: discovery can fail
	// at startup even though OIDC is configured.
	_, err := goth.GetProvider(oidcProviderName)

	return c.JSON(http.StatusOK, InstanceConfigResponse{
		OIDC: OIDCConfigResponse{
			Enabled:     err == nil,
			DisplayName: h.Config.Auth.OIDC.DisplayName,
		},
	})
}
