//go:build integration
// +build integration

package integration

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/go-jose/go-jose/v3"
	"github.com/labstack/gommon/log"
	"github.com/markbates/goth"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/config"
	"hopp-backend/internal/handlers"
	"hopp-backend/internal/models"
	"hopp-backend/internal/server"
)

const oidcTestClientID = "hopp-test-client"

const oidcTestKeyID = "test-key"

var (
	oidcTestKeysOnce sync.Once
	// oidcTestKey signs the fake provider's ID tokens and is published in its
	// key set; oidcForeignKey is a key the provider does not publish.
	oidcTestKey, oidcForeignKey *rsa.PrivateKey
)

func oidcTestKeys(t *testing.T) (*rsa.PrivateKey, *rsa.PrivateKey) {
	oidcTestKeysOnce.Do(func() {
		var err error
		oidcTestKey, err = rsa.GenerateKey(rand.Reader, 2048)
		require.NoError(t, err)
		oidcForeignKey, err = rsa.GenerateKey(rand.Reader, 2048)
		require.NoError(t, err)
	})
	return oidcTestKey, oidcForeignKey
}

// signIDToken returns the claims as a JWT signed with RS256.
func signIDToken(t *testing.T, key *rsa.PrivateKey, claims map[string]interface{}) string {
	signer, err := jose.NewSigner(
		jose.SigningKey{Algorithm: jose.RS256, Key: jose.JSONWebKey{Key: key, KeyID: oidcTestKeyID}},
		nil,
	)
	require.NoError(t, err)
	payload, err := json.Marshal(claims)
	require.NoError(t, err)
	signed, err := signer.Sign(payload)
	require.NoError(t, err)
	token, err := signed.CompactSerialize()
	require.NoError(t, err)
	return token
}

// fakeIDP is a minimal OpenID Connect provider for a public client: discovery,
// a key set and a token endpoint that enforces PKCE (S256).
type fakeIDP struct {
	server *httptest.Server
	// claims are merged into the ID token returned for the next login.
	claims map[string]interface{}
	// challenge is the code_challenge the client sent to the authorization
	// endpoint, which the test relays here like a real provider would store it.
	challenge string
	// tokenRequests counts successful token exchanges.
	tokenRequests int
	// mintIDToken overrides how the ID token is produced, to hand out tokens a
	// real provider never would.
	mintIDToken func(claims map[string]interface{}) string
}

func newFakeIDP(t *testing.T) *fakeIDP {
	idp := &fakeIDP{}
	signingKey, _ := oidcTestKeys(t)
	mux := http.NewServeMux()

	mux.HandleFunc("/.well-known/openid-configuration", func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]interface{}{
			"issuer":                 idp.server.URL,
			"authorization_endpoint": idp.server.URL + "/authorize",
			"token_endpoint":         idp.server.URL + "/token",
			"jwks_uri":               idp.server.URL + "/jwks",
		})
	})

	mux.HandleFunc("/jwks", func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(jose.JSONWebKeySet{Keys: []jose.JSONWebKey{
			{Key: &signingKey.PublicKey, KeyID: oidcTestKeyID, Algorithm: "RS256", Use: "sig"},
		}})
	})

	mux.HandleFunc("/token", func(w http.ResponseWriter, r *http.Request) {
		require.NoError(t, r.ParseForm())

		// Public client: no secret, the code verifier is the only proof.
		sum := sha256.Sum256([]byte(r.Form.Get("code_verifier")))
		if idp.challenge == "" || base64.RawURLEncoding.EncodeToString(sum[:]) != idp.challenge {
			http.Error(w, `{"error":"invalid_grant"}`, http.StatusBadRequest)
			return
		}
		if r.Form.Get("client_secret") != "" {
			http.Error(w, `{"error":"invalid_client"}`, http.StatusBadRequest)
			return
		}

		claims := map[string]interface{}{
			"iss": idp.server.URL,
			"aud": oidcTestClientID,
			"sub": "user-123",
			"exp": time.Now().Add(time.Hour).Unix(),
		}
		for k, v := range idp.claims {
			claims[k] = v
		}
		idToken := signIDToken(t, signingKey, claims)
		if idp.mintIDToken != nil {
			idToken = idp.mintIDToken(claims)
		}

		idp.tokenRequests++
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"access_token": "access-token",
			"token_type":   "Bearer",
			"expires_in":   3600,
			"id_token":     idToken,
		})
	})

	idp.server = httptest.NewServer(mux)
	t.Cleanup(idp.server.Close)
	return idp
}

func setupTestServerWithOIDC(t *testing.T, idp *fakeIDP, configure func(cfg *config.Config)) (*server.Server, func()) {
	cfg := &config.Config{}
	cfg.Server.Port = "8080"
	cfg.Server.Host = "localhost"
	cfg.Server.DeployDomain = "localhost:8080"
	cfg.Database.DSN = "file::memory:?cache=shared"
	cfg.Auth.SessionSecret = "test-secret-key-for-testing-only"
	cfg.Resend.DefaultSender = "test@example.com"
	cfg.Auth.OIDC.IssuerURL = idp.server.URL
	cfg.Auth.OIDC.ClientID = oidcTestClientID
	cfg.Auth.OIDC.DisplayName = "Test IdP"
	cfg.Auth.OIDC.Redirect = "https://localhost:8080/api/auth/social/oidc/callback"
	if configure != nil {
		configure(cfg)
	}

	srv := server.New(cfg)
	srv.Echo.Logger.SetLevel(log.OFF)
	require.NoError(t, srv.Initialize())

	cleanup := func() {
		goth.ClearProviders()
		if srv.DB != nil {
			if sqlDB, _ := srv.DB.DB(); sqlDB != nil {
				sqlDB.Close()
			}
		}
	}
	return srv, cleanup
}

// oidcLogin drives the full authorization code flow against the fake provider
// and returns the response of the callback.
func oidcLogin(t *testing.T, srv *server.Server, idp *fakeIDP, claims map[string]interface{}, tamper func(callback url.Values)) *httptest.ResponseRecorder {
	return oidcLoginFrom(t, srv, idp, "/api/auth/social/oidc", claims, tamper)
}

// oidcLoginFrom is oidcLogin with a custom start URL, e.g. one carrying a team
// invitation.
func oidcLoginFrom(t *testing.T, srv *server.Server, idp *fakeIDP, startURL string, claims map[string]interface{}, tamper func(callback url.Values)) *httptest.ResponseRecorder {
	idp.claims = claims

	// 1. Start the login, the backend redirects to the provider.
	req := httptest.NewRequest(http.MethodGet, startURL, nil)
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	require.Equal(t, http.StatusTemporaryRedirect, rec.Code, rec.Body.String())

	authURL, err := url.Parse(rec.Header().Get("Location"))
	require.NoError(t, err)
	assert.Equal(t, idp.server.URL+"/authorize", authURL.Scheme+"://"+authURL.Host+authURL.Path)
	assert.Equal(t, oidcTestClientID, authURL.Query().Get("client_id"))
	assert.Equal(t, "S256", authURL.Query().Get("code_challenge_method"))
	require.NotEmpty(t, authURL.Query().Get("code_challenge"))
	idp.challenge = authURL.Query().Get("code_challenge")

	// 2. The provider redirects back with a code and the state.
	callback := url.Values{}
	callback.Set("code", "auth-code")
	callback.Set("state", authURL.Query().Get("state"))
	if tamper != nil {
		tamper(callback)
	}
	req = httptest.NewRequest(http.MethodGet, "/api/auth/social/oidc/callback?"+callback.Encode(), nil)
	cookies := map[string]*http.Cookie{}
	for _, cookie := range rec.Result().Cookies() {
		cookies[cookie.Name] = cookie // the last Set-Cookie per name wins, as in a browser
	}
	for _, cookie := range cookies {
		req.AddCookie(cookie)
	}
	rec = httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	return rec
}

func verifiedClaims(email, givenName string) map[string]interface{} {
	return map[string]interface{}{
		"email":          email,
		"email_verified": true,
		"given_name":     givenName,
		"family_name":    "Tester",
	}
}

func getInstanceConfig(t *testing.T, srv *server.Server) handlers.InstanceConfigResponse {
	req := httptest.NewRequest(http.MethodGet, "/api/config", nil)
	rec := httptest.NewRecorder()
	srv.Echo.ServeHTTP(rec, req)
	require.Equal(t, http.StatusOK, rec.Code)
	var resp handlers.InstanceConfigResponse
	require.NoError(t, json.Unmarshal(rec.Body.Bytes(), &resp))
	return resp
}

func TestOIDC_NewUserSignsInWithPKCE(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	rec := oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), nil)

	require.Equal(t, http.StatusFound, rec.Code, rec.Body.String())
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")
	assert.Equal(t, 1, idp.tokenRequests)

	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "ada@gmail.com").First(&user).Error)
	assert.Equal(t, "Ada", user.FirstName)
	assert.Equal(t, "Tester", user.LastName)
	assert.True(t, user.IsAdmin)
	assert.NotNil(t, user.TeamID)
}

func TestOIDC_ExistingUserIsMatchedByEmail(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()
	existing := createTestUser(t, srv.DB, "ada@gmail.com", "Ada", "Existing", "securepassword123", false)

	rec := oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), nil)

	require.Equal(t, http.StatusFound, rec.Code, rec.Body.String())
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")

	var count int64
	require.NoError(t, srv.DB.Model(&models.User{}).Count(&count).Error)
	assert.Equal(t, int64(1), count)
	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "ada@gmail.com").First(&user).Error)
	assert.Equal(t, existing.ID, user.ID)
}

func TestOIDC_UnverifiedEmailRejected(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	for name, claims := range map[string]map[string]interface{}{
		"claim false":   {"email": "ada@gmail.com", "email_verified": false, "given_name": "Ada"},
		"claim missing": {"email": "ada@gmail.com", "given_name": "Ada"},
	} {
		t.Run(name, func(t *testing.T) {
			rec := oidcLogin(t, srv, idp, claims, nil)

			assert.Equal(t, http.StatusFound, rec.Code)
			assert.Equal(t, "/login?error=email_not_verified", rec.Header().Get("Location"))

			var count int64
			require.NoError(t, srv.DB.Model(&models.User{}).Count(&count).Error)
			assert.Equal(t, int64(0), count)
		})
	}
}

func TestOIDC_CallerSuppliedVerifierIsIgnored(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	// A code_verifier in the callback URL must not replace the one in the
	// session, otherwise the token exchange below would fail.
	rec := oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), func(callback url.Values) {
		callback.Set("code_verifier", "attacker-controlled")
	})

	assert.Equal(t, http.StatusFound, rec.Code, rec.Body.String())
	assert.Contains(t, rec.Header().Get("Location"), "/login?token=")
}

func TestOIDC_InvalidIDTokenSignatureRejected(t *testing.T) {
	_, foreignKey := oidcTestKeys(t)

	unsigned := func(claims map[string]interface{}) string {
		payload, _ := json.Marshal(claims)
		header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"none"}`))
		return header + "." + base64.RawURLEncoding.EncodeToString(payload) + "."
	}
	signedByForeignKey := func(claims map[string]interface{}) string {
		return signIDToken(t, foreignKey, claims)
	}
	tampered := func(claims map[string]interface{}) string {
		key, _ := oidcTestKeys(t)
		parts := strings.Split(signIDToken(t, key, claims), ".")
		claims["email"] = "someone.else@gmail.com"
		payload, _ := json.Marshal(claims)
		return parts[0] + "." + base64.RawURLEncoding.EncodeToString(payload) + "." + parts[2]
	}

	for name, mint := range map[string]func(map[string]interface{}) string{
		"unsigned token":          unsigned,
		"signed by a foreign key": signedByForeignKey,
		"payload changed":         tampered,
	} {
		t.Run(name, func(t *testing.T) {
			idp := newFakeIDP(t)
			srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
			defer cleanup()
			createTestUser(t, srv.DB, "ada@gmail.com", "Ada", "Existing", "securepassword123", false)
			idp.mintIDToken = mint

			rec := oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), nil)

			assert.Equal(t, http.StatusUnauthorized, rec.Code)
			assert.NotContains(t, rec.Header().Get("Location"), "token=")
		})
	}
}

func TestOIDC_InsecureEndpointsRejected(t *testing.T) {
	cfg := &config.Config{}
	cfg.Auth.OIDC.ClientID = oidcTestClientID

	// A plain http issuer is refused before any request is made.
	cfg.Auth.OIDC.IssuerURL = "http://idp.example.com"
	_, err := handlers.NewOIDCProvider(cfg)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "must use https")

	// So is a discovery document that points to a plain http endpoint.
	var issuer string
	idp := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]interface{}{
			"issuer":                 issuer,
			"authorization_endpoint": issuer + "/authorize",
			"token_endpoint":         "http://idp.example.com/token",
			"jwks_uri":               issuer + "/jwks",
		})
	}))
	defer idp.Close()
	issuer = idp.URL

	cfg.Auth.OIDC.IssuerURL = issuer
	_, err = handlers.NewOIDCProvider(cfg)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "token_endpoint must use https")
}

func TestOIDC_StateMismatchRejected(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	rec := oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), func(callback url.Values) {
		callback.Set("state", "forged")
	})

	assert.NotEqual(t, http.StatusFound, rec.Code)
	assert.Equal(t, 0, idp.tokenRequests)
}

func TestOIDC_DefaultCreatesOwnTeamPerUser(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	require.Equal(t, http.StatusFound, oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), nil).Code)
	require.Equal(t, http.StatusFound, oidcLogin(t, srv, idp, verifiedClaims("grace@gmail.com", "Grace"), nil).Code)

	var ada, grace models.User
	require.NoError(t, srv.DB.Where("email = ?", "ada@gmail.com").First(&ada).Error)
	require.NoError(t, srv.DB.Where("email = ?", "grace@gmail.com").First(&grace).Error)
	assert.NotEqual(t, *ada.TeamID, *grace.TeamID)
	assert.True(t, grace.IsAdmin)
}

func TestOIDC_SingleTeamJoinsFirstTeam(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, func(cfg *config.Config) {
		cfg.Auth.OIDC.SingleTeam = true
	})
	defer cleanup()

	require.Equal(t, http.StatusFound, oidcLogin(t, srv, idp, verifiedClaims("ada@gmail.com", "Ada"), nil).Code)
	require.Equal(t, http.StatusFound, oidcLogin(t, srv, idp, verifiedClaims("grace@gmail.com", "Grace"), nil).Code)

	var ada, grace models.User
	require.NoError(t, srv.DB.Where("email = ?", "ada@gmail.com").First(&ada).Error)
	require.NoError(t, srv.DB.Where("email = ?", "grace@gmail.com").First(&grace).Error)

	// The first user bootstraps the team as admin, everyone else joins it.
	assert.True(t, ada.IsAdmin)
	assert.False(t, grace.IsAdmin)
	assert.Equal(t, *ada.TeamID, *grace.TeamID)

	var teams int64
	require.NoError(t, srv.DB.Model(&models.Team{}).Count(&teams).Error)
	assert.Equal(t, int64(1), teams)
}

func TestOIDC_InvitedUserJoinsInvitingTeam(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	createTestTeam(t, srv.DB, "Some Other Team")
	team := createTestTeam(t, srv.DB, "Inviting Team")
	invitation := models.TeamInvitation{TeamID: int(team.ID), UniqueID: "0199a000-0000-7000-8000-000000000001"}
	require.NoError(t, srv.DB.Create(&invitation).Error)

	rec := oidcLoginFrom(t, srv, idp, "/api/auth/social/oidc?invite_uuid="+invitation.UniqueID, verifiedClaims("ada@gmail.com", "Ada"), nil)
	require.Equal(t, http.StatusFound, rec.Code, rec.Body.String())

	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "ada@gmail.com").First(&user).Error)
	assert.Equal(t, team.ID, *user.TeamID)
	assert.False(t, user.IsAdmin)
}

func TestOIDC_FirstNameFallsBackWithoutGivenName(t *testing.T) {
	idp := newFakeIDP(t)
	srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
	defer cleanup()

	rec := oidcLogin(t, srv, idp, map[string]interface{}{
		"email":              "ada@gmail.com",
		"email_verified":     true,
		"preferred_username": "ada.l",
	}, nil)
	require.Equal(t, http.StatusFound, rec.Code, rec.Body.String())

	var user models.User
	require.NoError(t, srv.DB.Where("email = ?", "ada@gmail.com").First(&user).Error)
	assert.Equal(t, "ada.l", user.FirstName)
}

func TestInstanceConfig_OIDC(t *testing.T) {
	t.Run("enabled when the provider is registered", func(t *testing.T) {
		idp := newFakeIDP(t)
		srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
		defer cleanup()

		resp := getInstanceConfig(t, srv)
		assert.True(t, resp.OIDC.Enabled)
		assert.Equal(t, "Test IdP", resp.OIDC.DisplayName)
	})

	t.Run("disabled when not configured", func(t *testing.T) {
		srv, cleanup := setupTestServerFast(t)
		defer cleanup()
		defer goth.ClearProviders()

		assert.False(t, getInstanceConfig(t, srv).OIDC.Enabled)
	})

	t.Run("disabled when discovery fails, server still starts", func(t *testing.T) {
		idp := newFakeIDP(t)
		idp.server.Close()
		srv, cleanup := setupTestServerWithOIDC(t, idp, nil)
		defer cleanup()

		assert.False(t, getInstanceConfig(t, srv).OIDC.Enabled)

		req := httptest.NewRequest(http.MethodGet, "/api/auth/social/oidc", nil)
		rec := httptest.NewRecorder()
		srv.Echo.ServeHTTP(rec, req)
		assert.Equal(t, http.StatusBadRequest, rec.Code)
	})
}
