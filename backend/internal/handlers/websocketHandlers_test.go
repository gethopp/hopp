package handlers

import (
	"context"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"hopp-backend/internal/common"
	"hopp-backend/internal/models"

	"github.com/alicebob/miniredis/v2"
	"github.com/golang-jwt/jwt/v5"
	"github.com/gorilla/websocket"
	"github.com/labstack/echo/v4"
	"github.com/redis/go-redis/v9"
	"github.com/stretchr/testify/require"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
)

func TestWebSocketShutdown(t *testing.T) {
	for _, shutdown := range []string{"websocket", "redis", "request", "simultaneous"} {
		t.Run(shutdown, func(t *testing.T) {
			db, err := gorm.Open(sqlite.Open(":memory:"), &gorm.Config{})
			require.NoError(t, err)
			sqlDB, err := db.DB()
			require.NoError(t, err)
			t.Cleanup(func() { sqlDB.Close() })
			require.NoError(t, db.Table("users").AutoMigrate(&models.UserProfile{}))
			user := models.UserProfile{ID: "test-user", FirstName: "Test", Email: "test@example.com"}
			require.NoError(t, db.Table("users").Create(&user).Error)

			mr := miniredis.RunT(t)
			rdb := redis.NewClient(&redis.Options{Addr: mr.Addr()})
			t.Cleanup(func() { rdb.Close() })
			e := echo.New()
			e.Logger.SetOutput(io.Discard)
			handler := CreateWSHandler(&common.ServerState{DB: db, Redis: rdb, JwtIssuer: NewJwtAuth("test-secret")})
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			returned := make(chan error, 1)
			e.GET("/websocket", func(c echo.Context) error {
				c.Set("user", &jwt.Token{Claims: &common.JwtCustomClaims{Email: user.Email}})
				c.SetRequest(c.Request().WithContext(ctx))
				err := handler(c)
				returned <- err
				return err
			})
			server := httptest.NewServer(e)
			t.Cleanup(server.Close)
			ws, _, err := websocket.DefaultDialer.Dial("ws"+strings.TrimPrefix(server.URL, "http")+"/websocket", nil)
			require.NoError(t, err)
			t.Cleanup(func() { ws.Close() })
			require.NoError(t, ws.SetReadDeadline(time.Now().Add(3*time.Second)))
			_, _, err = ws.ReadMessage()
			require.NoError(t, err)
			require.Eventually(t, func() bool {
				channels, err := rdb.PubSubChannels(context.Background(), user.GetRedisChannel()).Result()
				return err == nil && len(channels) == 1
			}, 3*time.Second, time.Millisecond)

			switch shutdown {
			case "websocket":
				require.NoError(t, ws.Close())
			case "redis":
				require.NoError(t, rdb.Close())
			case "request":
				cancel()
			case "simultaneous":
				start := make(chan struct{})
				go func() {
					<-start
					ws.Close()
				}()
				close(start)
				require.NoError(t, rdb.Close())
			}

			select {
			case err := <-returned:
				require.NoError(t, err)
			case <-time.After(3 * time.Second):
				t.Fatal("websocket handler did not return after shutdown")
			}
			if shutdown == "redis" || shutdown == "request" {
				_, _, err := ws.ReadMessage()
				require.True(t, websocket.IsUnexpectedCloseError(err), "server did not close websocket: %v", err)
			}
		})
	}
}

func TestConsumePendingInviteRequiresExactInvite(t *testing.T) {
	mr := miniredis.RunT(t)
	rdb := redis.NewClient(&redis.Options{Addr: mr.Addr()})
	t.Cleanup(func() { rdb.Close() })

	key := pendingInviteKey("inviter", "invitee", "current")
	if err := rdb.Set(context.Background(), key, "1", 0).Err(); err != nil {
		t.Fatal(err)
	}
	if consumePendingInvite(rdb, "inviter", "invitee", "stale") {
		t.Fatal("stale invite consumed the current invite")
	}
	if !consumePendingInvite(rdb, "inviter", "invitee", "current") {
		t.Fatal("matching invite was not consumed")
	}
	if consumePendingInvite(rdb, "inviter", "invitee", "current") {
		t.Fatal("invite was consumed twice")
	}
}
