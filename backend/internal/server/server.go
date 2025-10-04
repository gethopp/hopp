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
	"github.com/markbates/goth/providers/google"
	"github.com/markbates/goth/providers/slack"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/redis/go-redis/v9"
	resend "github.com/resend/resend-go/v2"
	"github.com/wader/gormstore/v2"
	"gorm.io/driver/postgres"
	"gorm.io/gorm"
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

	db, err := gorm.Open(postgres.Open(dsn), &gorm.Config{TranslateError: true})
	if err != nil {
		s.Echo.Logger.Fatal(err)
	}
	s.DB = db
}

func (s *Server) setupRedis() {

	url := s.Config.Database.RedisURI

	opts, err := redis.ParseURL(url)
	if err != nil {
		panic(err)
	}

	s.Redis = redis.NewClient(opts)

	// Validate proper connection
	ctx := context.Background()
	result := s.Redis.Ping(ctx)
	if result.Err() != nil {
		panic(result.Err())
	}
}

func (s *Server) setupSessionStore() {
	store := gormstore.New(s.DB, []byte(s.Config.Auth.SessionSecret))
	store.SessionOpts.MaxAge = 60 * 60 * 24 * 30 // 30 days
	quit := make(chan struct{})
	go store.PeriodicCleanup(1*time.Hour, quit)

	// To solve securecookie: error - caused by: gob: type not registered for interface
	gob.Register(map[string]interface{}{})

	s.Store = store
}

func (s *Server) setupTemplates() {
	t := &Template{
		templates: template.Must(template.ParseGlob("./web/*.html")),
	}
	s.Echo.Renderer = t
}

func (s *Server) runMigrations() {
	err := s.DB.AutoMigrate(
		&models.User{},
		&models.Team{},
		&models.TeamInvitation{},
		&models.EmailInvitation{},
		&models.Subscription{},
	)
	if err != nil {
		s.Echo.Logger.Fatal(err)
	}
}

func (s *Server) setupMiddleware() {
	s.Echo.Use(middleware.CORS())
	s.Echo.Use(session.Middleware(s.Store))
	s.Echo.Use(middleware.Recover())
	s.Echo.Use(echoprometheus.NewMiddleware("renkey_backend"))
}

func (s *Server) setupMetrics() {
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
	auth := handlers.NewAuthHandler(s.DB, s.Config, s.JwtIssuer, s.Redis)
	billing := handlers.NewBillingHandler(s.DB, s.Config, s.JwtIssuer, s.EmailClient)

	// Set the EmailClient field directly
	auth.ServerState.EmailClient = s.EmailClient

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
	api.GET("/watercooler/meet-redirect", auth.WatercoolerMeetRedirect)

	// Protected API routes group
	protectedAPI := api.Group("/auth", s.JwtIssuer.Middleware())

	protectedAPI.GET("/authenticate-app", auth.AuthenticateApp)
	protectedAPI.GET("/user", auth.User)
	protectedAPI.PUT("/update-user-name", auth.UpdateName)
	protectedAPI.GET("/teammates", auth.Teammates)
	protectedAPI.GET("/websocket", handlers.CreateWSHandler(&s.ServerState))
	protectedAPI.GET("/get-invite-uuid", auth.GetInviteUUID)
	protectedAPI.POST("/change-team/:uuid", auth.ChangeTeam)
	protectedAPI.POST("/send-team-invites", auth.SendTeamInvites)
	protectedAPI.POST("/metadata/onboarding-form", auth.UpdateOnboardingFormStatus)
	// Temporary room functionality for alpha
	// on-boarding of >2 people calls
	protectedAPI.GET("/watercooler", auth.Watercooler)
	protectedAPI.GET("/watercooler/anonymous", auth.WatercoolerAnonymous)

	// LiveKit server endpoint
	protectedAPI.GET("/livekit/server-url", auth.GetLivekitServerURL)

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
