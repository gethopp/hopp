package server

import (
	"context"
	"encoding/gob"
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/config"
	"hopp-backend/internal/email"
	"hopp-backend/internal/handlers"
	"hopp-backend/internal/models"
	"html/template"
	"io"
	"math"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/go-playground/validator"
	"github.com/labstack/echo-contrib/echoprometheus"
	"github.com/labstack/echo-contrib/session"
	"github.com/labstack/echo/v4"
	"github.com/labstack/echo/v4/middleware"
	"github.com/labstack/gommon/log"
	"github.com/markbates/goth"
	"github.com/markbates/goth/gothic"
	"github.com/markbates/goth/providers/github"
	"github.com/markbates/goth/providers/google"
	"github.com/markbates/goth/providers/slack"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/redis/go-redis/v9"
	resend "github.com/resend/resend-go/v2"
	"github.com/wader/gormstore/v2"
	"gorm.io/driver/postgres"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
	"gorm.io/gorm/logger"
)

// CustomValidator Source: https://echo.labstack.com/docs/request#validate-data
type CustomValidator struct {
	validator *validator.Validate
}

func (cv *CustomValidator) Validate(i interface{}) error {
	if err := cv.validator.Struct(i); err != nil {
		// Optionally, you could return the error to give each route more control over the status code
		return err
	}
	return nil
}

type Template struct {
	templates *template.Template
}

func (t *Template) Render(w io.Writer, name string, data interface{}, c echo.Context) error {
	return t.templates.ExecuteTemplate(w, name, data)
}

type SentryLogger struct {
	echo.Logger
}

func (l *SentryLogger) Error(i ...interface{}) {
	// Capture in Sentry
	if err, ok := i[0].(error); ok {
		handlers.CaptureError(err)
	} else {
		handlers.CaptureError(fmt.Errorf("%v", i...))
	}
	// Call original logger
	l.Logger.Error(i...)
}

type Server struct {
	common.ServerState
}

func New(cfg *config.Config) *Server {
	e := echo.New()
	e.Validator = &CustomValidator{validator: validator.New()}
	e.Logger = &SentryLogger{Logger: e.Logger}
	e.Logger.SetLevel(log.DEBUG)

	return &Server{
		common.ServerState{
			Echo:   e,
			Config: cfg,
		},
	}
}

func (s *Server) Initialize() error {
	// Initialize database
	s.setupDatabase()

	s.setupRedis()

	// Initialize JWT
	s.JwtIssuer = handlers.NewJwtAuth(s.Config.Auth.SessionSecret)

	// Initialize Resend email client
	s.setupEmailClient()

	// Initialize session store
	s.setupSessionStore()

	// Setup templates
	s.setupTemplates()

	// Setup routes
	s.setupRoutes()

	// Run Migrations
	s.runMigrations()

	// Setup goth providers
	s.setupGothProviders()

	s.setupMetrics()

	// Setup middleware -
	// Keep last to avoid Recover middleware and panic if something goes wrong on init
	s.setupMiddleware()

	return nil
}

func (s *Server) setupDatabase() {
	dsn := s.Config.Database.DSN
	if dsn == "" {
		s.Echo.Logger.Fatal("DATABASE_DSN environment variable is required")
	}

	var db *gorm.DB
	var err error

	// Detect database driver from DSN
	// SQLite DSNs typically start with "file:"
	if strings.HasPrefix(dsn, "file:") {
		// Use SQLite driver for testing
		db, err = gorm.Open(sqlite.Open(dsn), &gorm.Config{TranslateError: true, Logger: logger.Default.LogMode(logger.Silent)})
	} else {
		// Use PostgreSQL driver for production
		db, err = gorm.Open(postgres.Open(dsn), &gorm.Config{TranslateError: true, Logger: logger.Default.LogMode(logger.Silent)})
	}

	if err != nil {
		s.Echo.Logger.Fatal(err)
	}
	s.DB = db
}

func (s *Server) setupRedis() {
	url := s.Config.Database.RedisURI

	// Make Redis optional - if URI is empty, skip Redis setup
	if url == "" {
		s.Echo.Logger.Warn("REDIS_URI not configured, Redis features will be disabled")
		s.Redis = nil
		return
	}

	opts, err := redis.ParseURL(url)
	if err != nil {
		s.Echo.Logger.Warnf("Failed to parse Redis URL: %v, Redis features will be disabled", err)
		s.Redis = nil
		return
	}

	s.Redis = redis.NewClient(opts)

	// Validate proper connection, but don't panic on failure
	ctx := context.Background()
	result := s.Redis.Ping(ctx)
	if result.Err() != nil {
		s.Echo.Logger.Warnf("Redis connection failed: %v, Redis features will be disabled", result.Err())
		s.Redis = nil
		return
	}
}

func (s *Server) setupSessionStore() {
	store := gormstore.New(s.DB, []byte(s.Config.Auth.SessionSecret))
	store.SessionOpts.MaxAge = 60 * 60 * 24 * 30 // 30 days
	store.SessionOpts.SameSite = http.SameSiteLaxMode
	store.SessionOpts.HttpOnly = true

	quit := make(chan struct{})
	go store.PeriodicCleanup(1*time.Hour, quit)

	// To solve securecookie: error - caused by: gob: type not registered for interface
	gob.Register(map[string]interface{}{})

	s.Store = store
}

func (s *Server) setupTemplates() {
	// Try to load templates, but don't fail if they don't exist (e.g., in tests)
	tmpl, err := template.ParseGlob("./web/*.html")
	if err != nil {
		s.Echo.Logger.Warnf("Failed to load templates: %v, template rendering will be disabled", err)
		return
	}
	t := &Template{
		templates: tmpl,
	}
	s.Echo.Renderer = t
}

func (s *Server) runMigrations() {
	err := s.DB.AutoMigrate(
		&models.User{},
		&models.Team{},
		&models.Room{},
		&models.ResetToken{},
		&models.TeamInvitation{},
		&models.EmailInvitation{},
		&models.Subscription{},
		&models.Feedback{},
	)
	if err != nil {
		s.Echo.Logger.Fatal(err)
	}
}

func (s *Server) setupMiddleware() {
	s.Echo.Use(middleware.CORS())
	s.Echo.Use(session.Middleware(s.Store))
	s.Echo.Use(middleware.Recover())
	// Try to add prometheus middleware, but don't panic if already registered (e.g., in tests)
	// This allows multiple test runs without panicking
	defer func() {
		if r := recover(); r != nil {
			if err, ok := r.(error); ok && err.Error() == "duplicate metrics collector registration attempted" {
				s.Echo.Logger.Warn("Prometheus middleware already registered, skipping")
			} else {
				panic(r)
			}
		}
	}()
	s.Echo.Use(echoprometheus.NewMiddleware("renkey_backend"))
}

func (s *Server) setupMetrics() {
	// Only register Redis metrics if Redis is available
	if s.Redis == nil {
		return
	}

	prometheus.MustRegister(prometheus.NewGaugeFunc(
		prometheus.GaugeOpts{
			Subsystem: "redis",
			Name:      "connected_clients",
			Help:      "The number of clients currently connected to Redis",
		},
		func() float64 {
			ctx := context.Background()
			connectedClientsRaw := s.Redis.InfoMap(ctx).Item("Clients", "connected_clients")

			connectedClients, err := strconv.ParseFloat(connectedClientsRaw, 64)
			if err != nil {
				return math.NaN()
			}

			return connectedClients
		},
	))
}

func (s *Server) setupGothProviders() {
	// Set the session secret for Goth
	gothic.Store = s.Store

	goth.UseProviders(
		google.New(s.Config.Auth.GoogleKey, s.Config.Auth.GoogleSecret, s.Config.Auth.GoogleRedirect, "email", "profile", "openid"),
		slack.New(s.Config.Auth.SlackKey, s.Config.Auth.SlackSecret, s.Config.Auth.SlackRedirect, "users:read", "users:read.email", "team:read"),
		github.New(s.Config.Auth.GitHubKey, s.Config.Auth.GitHubSecret, s.Config.Auth.GitHubRedirect, "user:email", "read:user"),
	)
}

func (s *Server) setupEmailClient() {
	apiKey := s.Config.Resend.APIKey
	if apiKey == "" {
		s.Echo.Logger.Warn("RESEND_API_KEY not configured, email notifications will be disabled")
		return
	}

	resendClient := resend.NewClient(apiKey)
	s.EmailClient = email.NewResendEmailClient(resendClient,
		s.Config.Resend.DefaultSender,
		s.Echo.Logger)
}

func (s *Server) setupRoutes() {
	handlers.SetupSentry(s.Echo, s.Config)

	// Serve static files
	s.Echo.Static("/static", "web/static")

	// Initialize handlers
	auth := handlers.NewAuthHandler(s.DB, s.Config, s.JwtIssuer, s.Redis, &handlers.RealGothicProvider{})
	billing := handlers.NewBillingHandler(s.DB, s.Config, s.JwtIssuer, s.EmailClient)

	// Set the EmailClient field directly
	auth.EmailClient = s.EmailClient

	// API routes group
	api := s.Echo.Group("/api")

	// Public API endpoints
	api.GET("/health", func(c echo.Context) error {
		return c.String(200, "OK")
	})
	api.GET("/metrics", echoprometheus.NewHandler())
	// Add invitation details endpoint
	api.GET("/invitation-details/:uuid", auth.GetInvitationDetails)

	// Unsubscribe endpoints
	api.GET("/unsubscribe/:token", auth.UnsubscribeUser)
	api.POST("/unsubscribe/:token", auth.UnsubscribeUser)

	// Billing webhook endpoint (public)
	api.POST("/billing/webhook", billing.HandleWebhook)

	// Authentication endpoints
	api.GET("/auth/social/:provider", auth.SocialLogin)
	api.GET("/auth/social/:provider/callback", auth.SocialLoginCallback)
	api.POST("/sign-up", auth.ManualSignUp)
	api.POST("/sign-in", auth.ManualSignIn)
	api.POST("/forgot-password", auth.ForgotPassword)
	api.PATCH("/reset-password/:token", auth.ResetPassword)
	api.GET("/room/meet-redirect", auth.RoomMeetRedirect)

	// Protected API routes group
	protectedAPI := api.Group("/auth", s.JwtIssuer.Middleware())

	protectedAPI.GET("/authenticate-app", auth.AuthenticateApp)
	protectedAPI.GET("/user", auth.User)
	protectedAPI.PUT("/update-user-name", auth.UpdateName)
	protectedAPI.GET("/teammates", auth.Teammates)
	protectedAPI.DELETE("/teammates/:userId", auth.RemoveTeammate)

	protectedAPI.GET("/websocket", handlers.CreateWSHandler(&s.ServerState))
	protectedAPI.GET("/get-invite-uuid", auth.GetInviteUUID)
	protectedAPI.POST("/change-team/:uuid", auth.ChangeTeam)
	protectedAPI.POST("/send-team-invites", auth.SendTeamInvites)
	protectedAPI.POST("/metadata/onboarding-form", auth.UpdateOnboardingFormStatus)
	protectedAPI.POST("/subscribe-linux-waitlist", auth.SubscribeToLinuxWaitingList)

	protectedAPI.POST("/room", auth.CreateRoom)
	protectedAPI.PUT("/room/:id", auth.UpdateRoom)
	protectedAPI.DELETE("/room/:id", auth.DeleteRoom)
	protectedAPI.GET("/room/:id", auth.GetRoom)
	protectedAPI.GET("/rooms", auth.GetRooms)
	protectedAPI.GET("/room/anonymous", auth.RoomAnonymous)

	// LiveKit server endpoint
	protectedAPI.GET("/livekit/server-url", auth.GetLivekitServerURL)

	// Feedback endpoint
	protectedAPI.POST("/feedback", auth.SubmitFeedback)

	// Protected billing endpoints
	protectedAPI.GET("/billing/subscription", billing.GetSubscriptionStatus)
	protectedAPI.POST("/billing/create-checkout-session", billing.CreateCheckoutSession)
	protectedAPI.POST("/billing/create-portal-session", billing.CreatePortalSession)

	// Debug endpoints - only enabled when ENABLE_DEBUG_ENDPOINTS=true
	if s.Config.Server.Debug {
		api.GET("/debug", func(c echo.Context) error {
			return c.Render(http.StatusOK, "debug.html", nil)
		})
		api.GET("/call-token", auth.GenerateDebugCallToken)
		api.GET("/jwt-debug", func(c echo.Context) error {
			email := c.QueryParam("email")
			token, err := s.JwtIssuer.GenerateToken(email)
			if err != nil {
				return c.String(http.StatusInternalServerError, "Failed to generate token")
			}
			return c.JSON(http.StatusOK, map[string]string{
				"email": email,
				"token": token,
			})
		})
	}

	// SPA handler - serve index.html for all other routes
	s.Echo.GET("/*", func(c echo.Context) error {
		// Skip API routes
		if strings.HasPrefix(c.Request().URL.Path, "/api") {
			return echo.NewHTTPError(http.StatusNotFound, "API endpoint not found")
		}
		webAppPath := "web/web-app.html"
		if s.Config.Server.Debug {
			webAppPath = "web/web-app-debug.html"
		}
		return c.File(webAppPath)
	})
}

func (s *Server) Start() error {
	serverURL := s.Config.Server.Host + ":" + s.Config.Server.Port

	if s.Config.Server.TLS.Enabled {
		if _, err := os.Stat(s.Config.Server.TLS.CertFile); os.IsNotExist(err) {
			s.Echo.Logger.Warn("TLS certificate file not found, falling back to HTTP")
			return s.Echo.Start(serverURL)
		}
		if _, err := os.Stat(s.Config.Server.TLS.KeyFile); os.IsNotExist(err) {
			s.Echo.Logger.Warn("TLS key file not found, falling back to HTTP")
			return s.Echo.Start(serverURL)
		}
		return s.Echo.StartTLS(serverURL, s.Config.Server.TLS.CertFile, s.Config.Server.TLS.KeyFile)
	}

	return s.Echo.Start(serverURL)
}
