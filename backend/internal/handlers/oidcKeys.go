package handlers

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"sync"
	"time"

	"github.com/go-jose/go-jose/v3"
)

// oidcKeyRefreshInterval limits how often an unknown key ID may trigger a new
// download of the issuer's keys, so forged tokens cannot be used to hammer it.
const oidcKeyRefreshInterval = time.Minute

// oidcSigningAlgorithms are the asymmetric algorithms accepted for ID tokens.
// "none" and the HMAC family are deliberately absent.
var oidcSigningAlgorithms = map[string]bool{
	"RS256": true, "RS384": true, "RS512": true,
	"PS256": true, "PS384": true, "PS512": true,
	"ES256": true, "ES384": true, "ES512": true,
	"EdDSA": true,
}

// oidcKeys holds the signing keys of the configured issuer. It is set when the
// provider is registered, like goth's own package-level provider registry.
var oidcKeys *oidcKeySet

// oidcKeySet caches the issuer's JSON Web Key Set and verifies ID tokens
// against it.
type oidcKeySet struct {
	url    string
	client *http.Client

	mu        sync.Mutex
	keys      jose.JSONWebKeySet
	fetchedAt time.Time
}

// newOIDCKeySet downloads the key set once so a broken jwks_uri is noticed at
// startup rather than on the first login.
func newOIDCKeySet(jwksURL string, client *http.Client) (*oidcKeySet, error) {
	set := &oidcKeySet{url: jwksURL, client: client}
	if err := set.fetch(); err != nil {
		return nil, err
	}
	return set, nil
}

func (s *oidcKeySet) fetch() error {
	res, err := s.client.Get(s.url)
	if err != nil {
		return fmt.Errorf("key set request failed: %w", err)
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		return fmt.Errorf("key set request returned status %d", res.StatusCode)
	}

	var keys jose.JSONWebKeySet
	if err := json.NewDecoder(res.Body).Decode(&keys); err != nil {
		return fmt.Errorf("invalid key set: %w", err)
	}
	if len(keys.Keys) == 0 {
		return errors.New("key set is empty")
	}

	s.keys = keys
	s.fetchedAt = time.Now()
	return nil
}

// candidates returns the keys that may have signed a token with the given key
// ID. An unknown ID triggers one refresh, which covers key rotation.
func (s *oidcKeySet) candidates(keyID string) []jose.JSONWebKey {
	s.mu.Lock()
	defer s.mu.Unlock()

	lookup := func() []jose.JSONWebKey {
		if keyID == "" {
			return s.keys.Keys
		}
		return s.keys.Key(keyID)
	}

	keys := lookup()
	if len(keys) == 0 && time.Since(s.fetchedAt) > oidcKeyRefreshInterval {
		if err := s.fetch(); err == nil {
			keys = lookup()
		}
	}
	return keys
}

// verify checks that the ID token carries a valid signature by one of the
// issuer's keys. goth has already validated issuer, audience and expiry, but
// it does not verify the signature.
func (s *oidcKeySet) verify(idToken string) error {
	token, err := jose.ParseSigned(idToken)
	if err != nil {
		return fmt.Errorf("malformed ID token: %w", err)
	}
	if len(token.Signatures) != 1 {
		return errors.New("ID token must carry exactly one signature")
	}

	header := token.Signatures[0].Header
	if !oidcSigningAlgorithms[header.Algorithm] {
		return fmt.Errorf("ID token uses unsupported signing algorithm %q", header.Algorithm)
	}

	for _, key := range s.candidates(header.KeyID) {
		if key.Use != "" && key.Use != "sig" {
			continue
		}
		if !key.IsPublic() {
			continue
		}
		if _, err := token.Verify(key.Key); err == nil {
			return nil
		}
	}
	return errors.New("ID token signature does not match any key of the issuer")
}

// requireSecureOIDCURL rejects plain http for anything but loopback hosts.
// Tokens and keys are only trustworthy when they are fetched over TLS; http on
// localhost is allowed for development and tests.
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
		if host == "localhost" {
			return nil
		}
		if ip := net.ParseIP(host); ip != nil && ip.IsLoopback() {
			return nil
		}
	}
	return fmt.Errorf("%s must use https: %q", name, raw)
}
