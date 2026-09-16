package handlers

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/labstack/echo/v4"
)

// Turnstile action identifiers. The widget sends these via data-action and we
// verify them server-side so a token minted for one form can't be replayed on
// another. Keep these in sync with the web app's widget usage.
const (
	turnstileActionSignUp         = "signup"
	turnstileActionSignIn         = "signin"
	turnstileActionForgotPassword = "forgot_password"
)

// maxTurnstileTokenLength is Cloudflare's documented maximum token length. Longer
// input is rejected without hitting Siteverify.
const maxTurnstileTokenLength = 2048

// Overridable in tests to point Siteverify at an httptest server. The client
// carries a short timeout so a slow/unreachable Siteverify degrades to a
// retryable 503 instead of hanging the request.
var (
	turnstileSiteVerifyURL = "https://challenges.cloudflare.com/turnstile/v0/siteverify"
	turnstileHTTPClient    = &http.Client{Timeout: 5 * time.Second}
)

type turnstileVerifyResponse struct {
	Success     bool     `json:"success"`
	Hostname    string   `json:"hostname"`
	Action      string   `json:"action"`
	ChallengeTS string   `json:"challenge_ts"`
	ErrorCodes  []string `json:"error-codes"`
}

// verifyTurnstile validates a Turnstile token against Cloudflare's Siteverify
// API, checking success, the route-specific action, and the deploy hostname.
//
// When no secret is configured it is a no-op (returns nil) so OSS/self-host
// builds remain usable — including ignoring any turnstile_token a client happens
// to send. On transport failure it returns a retryable 503; on a missing,
// invalid, expired, reused, or mismatched token it returns a 400.
func (h *AuthHandler) verifyTurnstile(c echo.Context, token, expectedAction string) error {
	if !h.Config.IsTurnstileEnabled() {
		return nil
	}

	if token == "" {
		return echo.NewHTTPError(http.StatusBadRequest, "Captcha verification is required.")
	}
	if len(token) > maxTurnstileTokenLength {
		return echo.NewHTTPError(http.StatusBadRequest, "Captcha verification failed. Please try again.")
	}

	form := url.Values{}
	form.Set("secret", h.Config.Turnstile.SecretKey)
	form.Set("response", token)
	// I don't think it improves accuracy at all,
	// but keeping for extra data:
	// https://developers.cloudflare.com/turnstile/get-started/server-side-validation/#required-parameters
	if ip := c.RealIP(); ip != "" {
		form.Set("remoteip", ip)
	}

	ctx, cancel := context.WithTimeout(c.Request().Context(), 5*time.Second)
	defer cancel()

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, turnstileSiteVerifyURL, strings.NewReader(form.Encode()))
	if err != nil {
		c.Logger().Errorf("turnstile: failed to build siteverify request: %v", err)
		return echo.NewHTTPError(http.StatusServiceUnavailable, "Captcha verification is temporarily unavailable. Please try again.")
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	resp, err := turnstileHTTPClient.Do(req)
	if err != nil {
		c.Logger().Errorf("turnstile: siteverify request failed: %v", err)
		return echo.NewHTTPError(http.StatusServiceUnavailable, "Captcha verification is temporarily unavailable. Please try again.")
	}
	defer resp.Body.Close()

	var result turnstileVerifyResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		c.Logger().Errorf("turnstile: failed to decode siteverify response: %v", err)
		return echo.NewHTTPError(http.StatusServiceUnavailable, "Captcha verification is temporarily unavailable. Please try again.")
	}

	if !result.Success {
		c.Logger().Warnf("turnstile: verification failed: %v", result.ErrorCodes)
		return echo.NewHTTPError(http.StatusBadRequest, "Captcha verification failed. Please try again.")
	}

	if expectedAction != "" && result.Action != expectedAction {
		c.Logger().Warnf("turnstile: action mismatch: expected=%q got=%q", expectedAction, result.Action)
		return echo.NewHTTPError(http.StatusBadRequest, "Captcha verification failed. Please try again.")
	}

	if expectedHost := h.Config.Server.DeployDomain; expectedHost != "" && result.Hostname != expectedHost {
		c.Logger().Warnf("turnstile: hostname mismatch: expected=%q got=%q", expectedHost, result.Hostname)
		return echo.NewHTTPError(http.StatusBadRequest, "Captcha verification failed. Please try again.")
	}

	return nil
}
