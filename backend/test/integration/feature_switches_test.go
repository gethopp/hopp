//go:build integration
// +build integration

package integration

import (
	"testing"

	"github.com/labstack/gommon/log"
	"github.com/markbates/goth"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"hopp-backend/internal/config"
	"hopp-backend/internal/server"
)

// setupTestServerWithConfig builds a test server like setupTestServerFast, but
// lets the caller adjust the config before the server is initialized.
func setupTestServerWithConfig(t *testing.T, configure func(cfg *config.Config)) (*server.Server, func()) {
	cfg := &config.Config{}
	cfg.Server.Port = "8080"
	cfg.Server.Host = "localhost"
	cfg.Server.DeployDomain = "localhost:8080"
	cfg.Server.Debug = false
	cfg.Database.DSN = "file::memory:?cache=shared"
	cfg.Database.RedisURI = ""
	cfg.Auth.SessionSecret = "test-secret-key-for-testing-only"
	cfg.Resend.DefaultSender = "test@example.com"
	configure(cfg)

	srv := server.New(cfg)
	srv.Echo.Logger.SetLevel(log.ERROR)

	require.NoError(t, srv.Initialize())

	cleanup := func() {
		if srv.DB != nil {
			if sqlDB, _ := srv.DB.DB(); sqlDB != nil {
				sqlDB.Close()
			}
		}
	}

	return srv, cleanup
}

func TestSocialProviders_OnlyConfiguredAreRegistered(t *testing.T) {
	_, cleanup := setupTestServerWithConfig(t, func(cfg *config.Config) {
		cfg.Auth.GitHubKey = "key"
		cfg.Auth.GitHubSecret = "secret"
	})
	defer cleanup()

	providers := goth.GetProviders()
	assert.Len(t, providers, 1)
	assert.Contains(t, providers, "github")
}
