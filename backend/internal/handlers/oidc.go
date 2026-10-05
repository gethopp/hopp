package handlers

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"hopp-backend/internal/config"

	"github.com/labstack/echo-contrib/session"
	"github.com/labstack/echo/v4"
	"github.com/markbates/goth"
	"github.com/markbates/goth/gothic"
	"github.com/markbates/goth/providers/openidConnect"
	"golang.org/x/oauth2"
)

const (
	// oidcProviderName is the :provider value of the generic OpenID Connect
	// provider in /api/auth/social/:provider.
	oidcProviderName = "oidc"
	// oidcVerifierSessionKey holds the PKCE code verifier between the redirect
	// to the identity provider and the callback.
	oidcVerifierSessionKey = "oidc_pkce_verifier"
	oidcDiscoveryTimeout   = 10 * time.Second
)

// oidcDiscovery is the part of the issuer's discovery document we use.
type oidcDiscovery struct {
	Issuer             string `json:"issuer"`
	AuthEndpoint       string `json:"authorization_endpoint"`
	TokenEndpoint      string `json:"token_endpoint"`
	UserInfoEndpoint   string `json:"userinfo_endpoint"`
	EndSessionEndpoint string `json:"end_session_endpoint"`
	JWKSURI            string `json:"jwks_uri"`
}

// NewOIDCProvider builds the generic OpenID Connect provider from the issuer's
// discovery document and loads the issuer's signing keys. No client secret is
// needed for public clients, the authorization code is always protected with
// PKCE.
func NewOIDCProvider(cfg *config.Config) (goth.Provider, error) {
	oidc := cfg.Auth.OIDC

	if err := requireSecureOIDCURL("OIDC_ISSUER_URL", oidc.IssuerURL); err != nil {
		return nil, err
	}

	// Discovery is done here rather than by goth so it can time out instead of
	// blocking server start when the identity provider does not answer.
	client := &http.Client{Timeout: oidcDiscoveryTimeout}
	res, err := client.Get(oidc.IssuerURL + "/.well-known/openid-configuration")
	if err != nil {
		return nil, fmt.Errorf("discovery request failed: %w", err)
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("discovery request returned status %d", res.StatusCode)
	}

	var discovery oidcDiscovery
	if err := json.NewDecoder(res.Body).Decode(&discovery); err != nil {
		return nil, fmt.Errorf("invalid discovery document: %w", err)
	}
	if strings.TrimRight(discovery.Issuer, "/") != oidc.IssuerURL {
		return nil, fmt.Errorf("issuer %q in discovery document does not match OIDC_ISSUER_URL", discovery.Issuer)
	}

	endpoints := []struct {
		name, url string
		required  bool
	}{
		{"authorization_endpoint", discovery.AuthEndpoint, true},
		{"token_endpoint", discovery.TokenEndpoint, true},
		{"jwks_uri", discovery.JWKSURI, true},
		{"userinfo_endpoint", discovery.UserInfoEndpoint, false},
	}
	for _, endpoint := range endpoints {
		if endpoint.url == "" {
			if endpoint.required {
				return nil, fmt.Errorf("discovery document lacks %s", endpoint.name)
			}
			continue
		}
		if err := requireSecureOIDCURL(endpoint.name, endpoint.url); err != nil {
			return nil, err
		}
	}

	keys, err := newOIDCKeySet(discovery.JWKSURI, client)
	if err != nil {
		return nil, err
	}

	provider, err := openidConnect.NewCustomisedURL(
		oidc.ClientID, oidc.ClientSecret, oidc.Redirect,
		discovery.AuthEndpoint, discovery.TokenEndpoint, discovery.Issuer,
		discovery.UserInfoEndpoint, discovery.EndSessionEndpoint,
		"openid", "email", "profile",
	)
	if err != nil {
		return nil, err
	}
	provider.SetName(oidcProviderName)

	oidcKeys = keys
	return provider, nil
}

// verifyOIDCIDToken checks the signature of an ID token against the keys of
// the configured issuer.
func verifyOIDCIDToken(idToken string) error {
	if oidcKeys == nil {
		return errors.New("no OIDC signing keys loaded")
	}
	return oidcKeys.verify(idToken)
}

// beginOIDCAuth redirects to the identity provider like gothic.BeginAuthHandler,
// but adds a PKCE (S256) challenge and remembers the verifier in the session.
func beginOIDCAuth(c echo.Context) error {
	req, res := c.Request(), c.Response()

	authURL, err := gothic.GetAuthURL(res, req)
	if err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	// Kept in the application session, next to the team invite. gothic's own
	// session is replaced on every write, so it cannot hold a second value.
	verifier := oauth2.GenerateVerifier()
	sess, err := session.Get("session", c)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to load login session")
	}
	sess.Values[oidcVerifierSessionKey] = verifier
	if err := sess.Save(req, res); err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to store login session")
	}

	u, err := url.Parse(authURL)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Invalid authorization URL")
	}
	q := u.Query()
	q.Set("code_challenge", oauth2.S256ChallengeFromVerifier(verifier))
	q.Set("code_challenge_method", "S256")
	u.RawQuery = q.Encode()

	return c.Redirect(http.StatusTemporaryRedirect, u.String())
}

// addOIDCCodeVerifier moves the PKCE code verifier from the session into the
// callback request, where goth picks it up for the token exchange. Any
// code_verifier sent by the caller is discarded.
func addOIDCCodeVerifier(c echo.Context) {
	req := c.Request()
	q := req.URL.Query()
	q.Del("code_verifier")

	if sess, err := session.Get("session", c); err == nil {
		if verifier, ok := sess.Values[oidcVerifierSessionKey].(string); ok {
			q.Set("code_verifier", verifier)
			// Single use
			delete(sess.Values, oidcVerifierSessionKey)
			sess.Save(req, c.Response())
		}
	}

	req.URL.RawQuery = q.Encode()
}

// oidcEmailVerified reports whether the identity provider vouches for the
// user's email address via the standard email_verified claim.
func oidcEmailVerified(user goth.User) bool {
	switch verified := user.RawData["email_verified"].(type) {
	case bool:
		return verified
	case string:
		return verified == "true"
	default:
		return false
	}
}

// oidcFallbackFirstName picks a first name for providers that send no
// given_name claim.
func oidcFallbackFirstName(user goth.User) string {
	for _, candidate := range []string{user.Name, user.NickName} {
		if candidate != "" {
			return candidate
		}
	}
	name, _, _ := strings.Cut(user.Email, "@")
	return name
}
