package handlers

import (
	"cmp"
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"

	"hopp-backend/internal/config"

	"github.com/coreos/go-oidc/v3/oidc"
	"github.com/labstack/echo-contrib/session"
	"github.com/labstack/echo/v4"
	"github.com/markbates/goth"
	"github.com/markbates/goth/gothic"
	"github.com/markbates/goth/providers/openidConnect"
	"golang.org/x/oauth2"
	"gorm.io/gorm"
)

const (
	// oidcProviderName is the :provider value of the generic OpenID Connect
	// provider in /api/auth/social/:provider.
	oidcProviderName = "oidc"
	// oidcVerifierSessionKey holds the PKCE code verifier between the redirect
	// to the identity provider and the callback.
	oidcVerifierSessionKey = "oidc_pkce_verifier"
	// oidcHTTPTimeout bounds discovery at startup and later key downloads, so
	// an unresponsive identity provider cannot block the server.
	oidcHTTPTimeout = 10 * time.Second
	// oidcTeamSetupLockKey is the PostgreSQL advisory lock taken while the
	// OIDC team is looked up and created. The value is arbitrary but fixed.
	oidcTeamSetupLockKey int64 = 0x686f70706f696463
)

// oidcIDTokenVerifier checks ID tokens against the configured issuer's keys. It
// is set when the provider is registered, like goth's own provider registry.
var oidcIDTokenVerifier *oidc.IDTokenVerifier

// NewOIDCProvider builds the generic OpenID Connect provider from the issuer's
// discovery document. No client secret is needed for public clients, the
// authorization code is always protected with PKCE.
func NewOIDCProvider(cfg *config.Config) (goth.Provider, error) {
	oidcCfg := cfg.Auth.OIDC
	if err := requireSecureOIDCURL("OIDC_ISSUER_URL", oidcCfg.IssuerURL); err != nil {
		return nil, err
	}

	client := &http.Client{Timeout: oidcHTTPTimeout}
	discovered, err := oidc.NewProvider(oidc.ClientContext(context.Background(), client), oidcCfg.IssuerURL)
	if err != nil {
		return nil, fmt.Errorf("discovery failed: %w", err)
	}

	var metadata struct {
		JWKSURI            string `json:"jwks_uri"`
		EndSessionEndpoint string `json:"end_session_endpoint"`
	}
	if err := discovered.Claims(&metadata); err != nil {
		return nil, fmt.Errorf("invalid discovery document: %w", err)
	}

	endpoint := discovered.Endpoint()
	userInfoURL := discovered.UserInfoEndpoint()
	type namedURL struct{ name, url string }
	endpoints := []namedURL{
		{"authorization_endpoint", endpoint.AuthURL},
		{"token_endpoint", endpoint.TokenURL},
		{"jwks_uri", metadata.JWKSURI},
	}
	if userInfoURL != "" {
		endpoints = append(endpoints, namedURL{"userinfo_endpoint", userInfoURL})
	}
	for _, e := range endpoints {
		if err := requireSecureOIDCURL(e.name, e.url); err != nil {
			return nil, err
		}
	}

	provider, err := openidConnect.NewCustomisedURL(
		oidcCfg.ClientID, oidcCfg.ClientSecret, oidcCfg.Redirect,
		endpoint.AuthURL, endpoint.TokenURL, oidcCfg.IssuerURL,
		userInfoURL, metadata.EndSessionEndpoint,
		"openid", "email", "profile",
	)
	if err != nil {
		return nil, err
	}
	provider.SetName(oidcProviderName)

	oidcIDTokenVerifier = discovered.Verifier(&oidc.Config{ClientID: oidcCfg.ClientID})
	return provider, nil
}

// verifyOIDCIDToken checks the ID token's signature, issuer, audience and
// expiry against the configured issuer.
func verifyOIDCIDToken(ctx context.Context, rawIDToken string) error {
	if oidcIDTokenVerifier == nil {
		return errors.New("OIDC provider is not registered")
	}
	_, err := oidcIDTokenVerifier.Verify(ctx, rawIDToken)
	return err
}

// requireSecureOIDCURL rejects plain http for anything but loopback hosts, so
// tokens and keys are only fetched over TLS. http on localhost is allowed for
// development and tests.
func requireSecureOIDCURL(name, raw string) error {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" {
		return fmt.Errorf("%s is not a valid URL: %q", name, raw)
	}
	if u.Scheme == "https" {
		return nil
	}
	if u.Scheme == "http" {
		host := u.Hostname()
		if ip := net.ParseIP(host); host == "localhost" || (ip != nil && ip.IsLoopback()) {
			return nil
		}
	}
	return fmt.Errorf("%s must use https: %q", name, raw)
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
// code_verifier sent by the caller is discarded. It fails if the session holds
// no verifier or the verifier cannot be removed from it, so the caller can
// reject the callback before any token exchange.
func addOIDCCodeVerifier(c echo.Context) error {
	req := c.Request()
	q := req.URL.Query()
	q.Del("code_verifier")
	req.URL.RawQuery = q.Encode()

	sess, err := session.Get("session", c)
	if err != nil {
		return fmt.Errorf("loading login session: %w", err)
	}
	verifier, ok := sess.Values[oidcVerifierSessionKey].(string)
	if !ok || verifier == "" {
		return errors.New("login session holds no PKCE verifier")
	}

	// Single use
	delete(sess.Values, oidcVerifierSessionKey)
	if err := sess.Save(req, c.Response()); err != nil {
		return fmt.Errorf("removing PKCE verifier from login session: %w", err)
	}

	q.Set("code_verifier", verifier)
	req.URL.RawQuery = q.Encode()
	return nil
}

// lockOIDCTeamSetup serializes the lookup and creation of the OIDC team, so
// two first sign-ins at the same time cannot each create one. The lock is
// released when the transaction ends. SQLite, used in tests only, has no
// advisory locks.
func lockOIDCTeamSetup(tx *gorm.DB) error {
	if tx.Name() != "postgres" {
		return nil
	}
	return tx.Exec("SELECT pg_advisory_xact_lock(?)", oidcTeamSetupLockKey).Error
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
	localPart, _, _ := strings.Cut(user.Email, "@")
	return cmp.Or(user.Name, user.NickName, localPart)
}
