package handlers

import (
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"hopp-backend/internal/common"
	"hopp-backend/internal/config"

	"github.com/labstack/echo/v4"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// newTestContext builds a throwaway echo context for exercising handler helpers
// that only need a request/response and a logger.
func newTestContext() (echo.Context, *httptest.ResponseRecorder) {
	e := echo.New()
	req := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(""))
	rec := httptest.NewRecorder()
	return e.NewContext(req, rec), rec
}

func newTurnstileHandler(secret, deployDomain string) *AuthHandler {
	cfg := &config.Config{}
	cfg.Turnstile.SecretKey = secret
	cfg.Server.DeployDomain = deployDomain
	return &AuthHandler{ServerState: common.ServerState{Config: cfg}}
}

// setSiteVerify points the verifier at a local test server for the duration of a test.
func setSiteVerify(t *testing.T, handler http.HandlerFunc) {
	ts := httptest.NewServer(handler)
	t.Cleanup(ts.Close)
	old := turnstileSiteVerifyURL
	turnstileSiteVerifyURL = ts.URL
	t.Cleanup(func() { turnstileSiteVerifyURL = old })
}

func httpErrorCode(t *testing.T, err error) int {
	t.Helper()
	require.Error(t, err)
	he, ok := err.(*echo.HTTPError)
	require.Truef(t, ok, "expected *echo.HTTPError, got %T", err)
	return he.Code
}

func TestVerifyTurnstile_DisabledIgnoresToken(t *testing.T) {
	h := newTurnstileHandler("", "localhost:1926")
	c, _ := newTestContext()

	// Disabled mode accepts requests with any (or no) token and never calls Siteverify.
	require.NoError(t, h.verifyTurnstile(c, "", turnstileActionSignIn))
	require.NoError(t, h.verifyTurnstile(c, "a-client-supplied-token", turnstileActionSignIn))
}

func TestVerifyTurnstile_MissingToken(t *testing.T) {
	h := newTurnstileHandler("secret", "localhost:1926")
	c, _ := newTestContext()
	assert.Equal(t, http.StatusBadRequest, httpErrorCode(t, h.verifyTurnstile(c, "", turnstileActionSignIn)))
}

func TestVerifyTurnstile_Success(t *testing.T) {
	setSiteVerify(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"success":true,"hostname":"cracked.alexoglou.com","action":"signin"}`)
	})

	h := newTurnstileHandler("secret", "cracked.alexoglou.com")
	c, _ := newTestContext()
	require.NoError(t, h.verifyTurnstile(c, "token", turnstileActionSignIn))
}

func TestVerifyTurnstile_ActionMismatch(t *testing.T) {
	setSiteVerify(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"success":true,"hostname":"cracked.alexoglou.com","action":"signup"}`)
	})

	h := newTurnstileHandler("secret", "cracked.alexoglou.com")
	c, _ := newTestContext()
	assert.Equal(t, http.StatusBadRequest, httpErrorCode(t, h.verifyTurnstile(c, "token", turnstileActionSignIn)))
}

func TestVerifyTurnstile_HostnameMismatch(t *testing.T) {
	setSiteVerify(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"success":true,"hostname":"evil.example","action":"signin"}`)
	})

	h := newTurnstileHandler("secret", "cracked.alexoglou.com")
	c, _ := newTestContext()
	assert.Equal(t, http.StatusBadRequest, httpErrorCode(t, h.verifyTurnstile(c, "token", turnstileActionSignIn)))
}

func TestVerifyTurnstile_Failure(t *testing.T) {
	setSiteVerify(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"success":false,"error-codes":["timeout-or-duplicate"]}`)
	})

	h := newTurnstileHandler("secret", "localhost:1926")
	c, _ := newTestContext()
	assert.Equal(t, http.StatusBadRequest, httpErrorCode(t, h.verifyTurnstile(c, "token", turnstileActionSignIn)))
}

func TestVerifyTurnstile_TransportFailureReturns503(t *testing.T) {
	old := turnstileSiteVerifyURL
	// Port 1 refuses connections, simulating a Siteverify transport failure.
	turnstileSiteVerifyURL = "http://127.0.0.1:1/siteverify"
	t.Cleanup(func() { turnstileSiteVerifyURL = old })

	h := newTurnstileHandler("secret", "localhost:1926")
	c, _ := newTestContext()
	assert.Equal(t, http.StatusServiceUnavailable, httpErrorCode(t, h.verifyTurnstile(c, "token", turnstileActionSignIn)))
}
