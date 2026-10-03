package config

import "testing"

func TestLoad_DisablePasswordLoginRequiresProvider(t *testing.T) {
	t.Setenv("ENV_STACK", "")
	t.Setenv("SESSION_SECRET", "test-secret")
	t.Setenv("DISABLE_PASSWORD_LOGIN", "true")
	for _, name := range []string{"GOOGLE_KEY", "GOOGLE_SECRET", "SLACK_KEY", "SLACK_SECRET", "GITHUB_KEY", "GITHUB_SECRET"} {
		t.Setenv(name, "")
	}

	if _, err := Load(); err == nil {
		t.Fatal("expected Load to fail when password login is disabled and no provider is configured")
	}

	t.Setenv("GITHUB_KEY", "key")
	t.Setenv("GITHUB_SECRET", "secret")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("expected Load to succeed with a configured provider, got: %v", err)
	}
	if !cfg.Auth.DisablePasswordLogin {
		t.Fatal("expected DisablePasswordLogin to be set")
	}
}
