package common

import (
	"net/http"

	"hopp-backend/internal/config"
	"hopp-backend/internal/email"

	"github.com/golang-jwt/jwt/v5"
	"github.com/labstack/echo/v4"
	"github.com/markbates/goth"
	"github.com/redis/go-redis/v9"
	"github.com/wader/gormstore/v2"
	"gorm.io/gorm"
)

type JwtCustomClaims struct {
	Email string `json:"email"`
	jwt.RegisteredClaims
}

type JwtAuth struct {
	Secret string
	Claims JwtCustomClaims
}

type JWTIssuer interface {
	GenerateToken(email string) (string, error)
	Middleware() echo.MiddlewareFunc
	GetUserEmail(c echo.Context) (string, error)
}

type SocialAuthProvider interface {
	CompleteUserAuth(res http.ResponseWriter, req *http.Request) (goth.User, error)
}

type AuthHandler interface {
	SocialLoginCallback(c echo.Context) error
	SocialLogin(c echo.Context) error
	ManualSignUp(c echo.Context) error
	UserPage(c echo.Context) error
}

type LivekitTokenSet struct {
	AudioToken  string `json:"audioToken"`
	VideoToken  string `json:"videoToken"`
	CameraToken string `json:"cameraToken"`
	Participant string `json:"participant"`
}

type ServerState struct {
	Echo        *echo.Echo
	Config      *config.Config
	DB          *gorm.DB
	Store       *gormstore.Store
	JwtIssuer   JWTIssuer
	Redis       *redis.Client
	EmailClient email.EmailClient
}
