package config

import (
	"fmt"
	"os"

	"github.com/joho/godotenv"
)

type Config struct {
	Server struct {
		Port string
		Host string
		TLS  struct {
			Enabled  bool
			CertFile string
			KeyFile  string
		}
		DeployDomain string
		Debug        bool
	}
	Auth struct {
		GoogleKey      string
		GoogleSecret   string
		GoogleRedirect string
		SlackKey       string
		SlackSecret    string
		SlackRedirect  string
		CallbackURL    string
		SessionSecret  string
	}
	Livekit struct {
		APIKey    string
		Secret    string
		ServerURL string
	}
	Database struct {
		DSN      string
		RedisURI string
	}
	Telegram struct {
		BotToken string
		ChatID   string
	}
	Resend struct {
		APIKey        string
		DefaultSender string
	}
	Sentry struct {
		DSN string
	}
	EmailValidation struct {
		BlockDisposableEmails bool
		StrictMode           bool // If true, only allows business domains
	}
}

func Load() (*Config, error) {

	envStack := os.Getenv("ENV_STACK")

	if envStack != "" {
		filePath := "./env-files/.env." + envStack
		err := godotenv.Load(
			filePath)
		if err != nil {
			fmt.Printf("Error loading .env file: %s\n", err)
		}
	}

	// Load configuration from environment variables or config file
	// You might want to use viper here
	// Load configuration from environment variables or config file
	// You might want to use viper here
	c := &Config{}

	// Server configuration with environment variable support
	c.Server.Port = os.Getenv("SERVER_PORT")
	if c.Server.Port == "" {
		c.Server.Port = "1926"
	}

	c.Server.Host = os.Getenv("SERVER_HOST")
	if c.Server.Host == "" {
		c.Server.Host = "localhost"
	}

	c.Server.DeployDomain = os.Getenv("DEPLOY_DOMAIN")
	if c.Server.DeployDomain == "" {
		c.Server.DeployDomain = c.Server.Host + ":" + c.Server.Port
	}

	c.Server.Debug = os.Getenv("ENABLE_DEBUG_ENDPOINTS") == "true"

	// TLS Configuration
	useTLS := os.Getenv("USE_TLS")
	c.Server.TLS.Enabled = useTLS != "false" && useTLS != "0"
	c.Server.TLS.CertFile = "./certs/localhost.pem"
	c.Server.TLS.KeyFile = "./certs/localhost-key.pem"

	c.Auth.SessionSecret = os.Getenv("SESSION_SECRET")

	c.Auth.GoogleKey = os.Getenv("GOOGLE_KEY")
	c.Auth.GoogleSecret = os.Getenv("GOOGLE_SECRET")
	c.Auth.GoogleRedirect = fmt.Sprintf("https://%s/api/auth/social/google/callback", c.Server.DeployDomain)

	c.Auth.SlackKey = os.Getenv("SLACK_KEY")
	c.Auth.SlackSecret = os.Getenv("SLACK_SECRET")
	c.Auth.SlackRedirect = fmt.Sprintf("https://%s/api/auth/social/slack/callback", c.Server.DeployDomain)

	c.Database.DSN = os.Getenv("DATABASE_DSN")
	c.Database.RedisURI = os.Getenv("REDIS_URI")

	c.Livekit.APIKey = os.Getenv("LIVEKIT_API_KEY")
	c.Livekit.Secret = os.Getenv("LIVEKIT_API_SECRET")
	c.Livekit.ServerURL = os.Getenv("LIVEKIT_SERVER_URL")

	c.Telegram.BotToken = os.Getenv("TELEGRAM_BOT_TOKEN")
	c.Telegram.ChatID = os.Getenv("TELEGRAM_CHAT_ID")

	c.Resend.APIKey = os.Getenv("RESEND_API_KEY")
	c.Resend.DefaultSender = os.Getenv("RESEND_DEFAULT_SENDER")
	if c.Resend.DefaultSender == "" {
		c.Resend.DefaultSender = "noreply@gethopp.app"
	}

	c.Sentry.DSN = os.Getenv("SENTRY_DSN")

	// Email Validation Configuration
	c.EmailValidation.BlockDisposableEmails = os.Getenv("BLOCK_DISPOSABLE_EMAILS") != "false"
	c.EmailValidation.StrictMode = os.Getenv("EMAIL_STRICT_MODE") == "true"

	return c, nil
}
