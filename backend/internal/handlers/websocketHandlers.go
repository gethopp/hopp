package handlers

import (
	"context"
	"encoding/json"
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/livekitutil"
	"hopp-backend/internal/messages"
	"hopp-backend/internal/models"
	"hopp-backend/internal/notifications"
	"hopp-backend/internal/redisutil"
	"net/http"
	"time"

	"github.com/google/uuid"
	"github.com/gorilla/websocket"
	"github.com/labstack/echo/v4"
	"github.com/redis/go-redis/v9"
)

// https://github.com/gorilla/websocket/blob/main/examples/chat/client.go#L35
var wsUpgrader = websocket.Upgrader{
	ReadBufferSize:  1024,
	WriteBufferSize: 1024,
}

func init() {
	// Allow all origins
	wsUpgrader.CheckOrigin = func(r *http.Request) bool {
		return true
	}
}

func CreateWSHandler(server *common.ServerState) echo.HandlerFunc {
	return func(c echo.Context) error {
		ws, err := wsUpgrader.Upgrade(c.Response(), c.Request(), nil)
		if err != nil {
			return err
		}
		defer ws.Close()

		// Kill the connections after 2 heartbeats (30 seconds for old apps)
		// TODO: Modify after most users are upgraded to >1.0.15
		const wsReadTimeout = 65 * time.Second
		_ = ws.SetReadDeadline(time.Now().Add(wsReadTimeout))

		// Get user from context
		email, err := server.JwtIssuer.GetUserEmail(c)
		if err != nil {
			return err
		}

		user, err := models.GetUserByEmail(server.DB, email)
		if err != nil {
			return err
		}

		// Create a cancellable context that will be used to cleanup resources
		ctx, cancel := context.WithCancel(c.Request().Context())
		defer cancel()

		// Subscribe to Redis channel for user updates
		pubsub := server.Redis.Subscribe(ctx, user.GetRedisChannel())
		defer func() {
			pubsub.Close()
			cancel()
		}()

		// Successful connection message
		success := messages.NewSuccessMessage("Successful connection for user: " + user.FirstName)

		s, err := json.Marshal(success)
		if err != nil {
			c.Logger().Error(err)
		}
		err = ws.WriteMessage(websocket.TextMessage, s)
		if err != nil {
			c.Logger().Errorf("Error writing initial websocket message: %v", err)
			return err
		}

		// Use done channel to signal when the connection is closed
		done := make(chan struct{})

		// Send user online message to teammates on connection
		teammates, err := user.GetTeammates(server.DB)
		if err != nil {
			c.Logger().Error(err)
		} else {
			for _, teammate := range teammates {
				// Check if teammate is online
				channels, err := server.Redis.PubSubChannels(ctx, redisutil.GetUserChannel(teammate.ID)).Result()
				if err != nil {
					c.Logger().Error(err)
					continue
				}
				if len(channels) > 0 {
					c.Logger().Info("Notify teammate: ", teammate.ID, " that user: ", user.ID, " is online")
					publishTeammateOnlineMessage(c, server, user.ID, teammate.ID)
				}
			}
		}

		// Websocket read loop
		go func() {
			defer func() {
				close(done)
				cancel() // Cancel context when websocket closes
			}()
			for {
				messageType, msg, err := ws.ReadMessage()
				if err != nil {
					if websocket.IsCloseError(err, websocket.CloseGoingAway, websocket.CloseAbnormalClosure, websocket.CloseNoStatusReceived) {
						c.Logger().Debug("WebSocket connection closed normally")
					} else {
						c.Logger().Errorf("WebSocket read error: %v (user: %s)", err, user.ID)
					}
					done <- struct{}{}
					return
				}

				if messageType != websocket.TextMessage {
					c.Logger().Warn("Received non-text message in websocket")
					continue
				}

				parsedMessage, err := messages.ParseMessage(msg)
				if err != nil {
					sendWSErrorMessage(ws, err.Error())
					continue
				}

				switch {
				case parsedMessage.CallRequest != nil:
					// Handle call request
					c.Logger().Info("Received call request")
					initiateCall(c, server, ws, pubsub, user.ID, parsedMessage.CallRequest.Payload.CalleeID)
				case parsedMessage.AcceptCallMessage != nil:
					// Handle call accept
					c.Logger().Info("Accepting call")
					acceptCall(c, server, user.ID, *parsedMessage.AcceptCallMessage)
				case parsedMessage.RejectCallMessage != nil:
					// Handle call end
					c.Logger().Info("Rejecting call")
					rejectCall(c, server, user.ID, *parsedMessage.RejectCallMessage)
				case parsedMessage.CallEnd != nil:
					// Handle call end
					c.Logger().Info("Ending call")
					endCall(c, server, user.ID, *parsedMessage.CallEnd)
				case parsedMessage.Ping != nil:
					// Handle ping message
					// Reset the read deadline
					_ = ws.SetReadDeadline(time.Now().Add(wsReadTimeout))
					c.Logger().Debugf("Received ping from user: %s", user.ID)
					pong := messages.NewPongMessage()
					pongJSON, err := json.Marshal(pong)
					if err != nil {
						c.Logger().Error(err)
						return
					}
					err = ws.WriteMessage(websocket.TextMessage, pongJSON)
					if err != nil {
						c.Logger().Error(err)
						return
					}
				case parsedMessage.TeammateOnlineMessage != nil:
					// Handle user online message
					c.Logger().Info("Received user online message ", parsedMessage.TeammateOnlineMessage.Payload.TeammateID, " ", user.ID)
					publishTeammateOnlineMessage(c, server, user.ID, parsedMessage.TeammateOnlineMessage.Payload.TeammateID)
				case parsedMessage.PresenceAck != nil:
					// Client confirmed (or denied) presence in a candidate room
					handlePresenceAck(c, server, user.ID, *parsedMessage.PresenceAck)
				default:
					c.Logger().Warn("Unknown message type")
				}

			}
		}()

		// Redis message loop
		go func() {
			defer cancel() // Ensure context is cancelled if this goroutine exits first
			for {
				select {
				case <-ctx.Done():
					return
				case <-done:
					c.Logger().Warnf("Redis subscription closed for user: %s\n", user.FirstName)
					return
				default:
					msg, err := pubsub.ReceiveMessage(ctx)
					if err != nil {
						select {
						case <-ctx.Done():
							// Context was cancelled, this is normal shutdown
							return
						default:
							if err == redis.ErrClosed {
								done <- struct{}{}
								return
							}
							// Only log truly unexpected errors
							if err.Error() != "use of closed network connection" {
								c.Logger().Error("Unexpected Redis error: ", err)
							}
							done <- struct{}{}
							return
						}
					}

					parsedMessage, err := messages.ParseMessage([]byte(msg.Payload))
					if err != nil {
						c.Logger().Error(err)
						continue
					}

					switch {
					case parsedMessage.IncomingCall != nil:
						// Forward incoming call message to the callee
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.RejectCallMessage != nil:
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.AcceptCallMessage != nil:
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.CallTokensMessage != nil:
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.CallEnd != nil:
						// Handle call end
						c.Logger().Info("Received call end")
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.TeammateOnlineMessage != nil:
						// Handle user online message
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.PresenceChanged != nil:
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					case parsedMessage.PresenceCheck != nil:
						err = ws.WriteMessage(websocket.TextMessage, []byte(msg.Payload))
						if err != nil {
							c.Logger().Error(err)
						}
					default:
						c.Logger().Warn("Unknown message type")
					}
				}
			}
		}()

		// Wait for connection to close
		<-done

		return nil
	}
}

func sendWSErrorMessage(ws *websocket.Conn, message string) {
	msg := messages.NewErrorMessage(message)
	msgJSON, err := json.Marshal(msg)
	if err != nil {
		return
	}
	ws.WriteMessage(websocket.TextMessage, msgJSON)
}

func dedupeCallKey(a, b string) string {
	if a < b {
		return "call:pending:" + a + ":" + b
	}
	return "call:pending:" + b + ":" + a
}

func initiateCall(ctx echo.Context, s *common.ServerState, ws *websocket.Conn, rdb *redis.PubSub, callerId, calleeID string) {
	rdbCtx := context.Background()
	calleeChannelID := redisutil.GetUserChannel(calleeID)
	dedupeKey := dedupeCallKey(callerId, calleeID)

	// Dedupe: prevent duplicate/glare call requests within ringing window.
	// Symmetric key: pending A->B also blocks B->A.
	const callDedupeTTL = 30 * time.Second
	acquired, err := s.Redis.SetNX(rdbCtx, dedupeKey, "1", callDedupeTTL).Result()
	if err != nil {
		// Fail open — do not block legit calls on Redis hiccup.
		ctx.Logger().Error("dedupe SETNX error: ", err)
	} else if !acquired {
		ctx.Logger().Warn("Duplicate call dropped: ", callerId, " -> ", calleeID)
		msg := messages.NewRejectCallMessage(calleeID, "already-calling")
		msgJSON, mErr := json.Marshal(msg)
		if mErr != nil {
			ctx.Logger().Error(mErr)
			return
		}
		ws.WriteMessage(websocket.TextMessage, msgJSON)
		return
	}

	// Check if the caller's team is in trial or paid tier
	caller, err := models.GetUserByID(s.DB, callerId)
	if err != nil {
		ctx.Logger().Error("Error getting caller: ", err)
		s.Redis.Del(rdbCtx, dedupeKey)
		sendWSErrorMessage(ws, "Failed to get caller information")
		return
	}

	// Check if caller has access (paid or active trial)
	hasAccess, err := checkUserHasAccess(s.DB, caller, s.Config.IsStripeEnabled())
	if err != nil {
		ctx.Logger().Error("Error getting caller subscription: ", err)
		s.Redis.Del(rdbCtx, dedupeKey)
		sendWSErrorMessage(ws, "Failed to check subscription status")
		return
	}

	if !hasAccess {
		ctx.Logger().Warn("Caller does not have active subscription or trial: ", callerId)
		s.Redis.Del(rdbCtx, dedupeKey)
		msg := messages.NewRejectCallMessage(calleeID, "trial-ended")
		msgJSON, err := json.Marshal(msg)
		if err != nil {
			ctx.Logger().Error("Error marshalling reject message: ", err)
			return
		}
		ws.WriteMessage(websocket.TextMessage, msgJSON)
		_ = notifications.SendTelegramNotification(fmt.Sprintf("Unsubscribed user %s tried to call", caller.ID), s.Config)
		return
	}

	// Check first if the callee online
	channels, err := s.Redis.PubSubChannels(rdbCtx, calleeChannelID).Result()
	if err != nil {
		ctx.Logger().Error("Error getting channels: %v", err)
		s.Redis.Del(rdbCtx, dedupeKey)
		return
	}

	if len(channels) == 0 {
		s.Redis.Del(rdbCtx, dedupeKey)
		msg := messages.NewCalleeOfflineMessage(calleeID)
		msgJSON, err := json.Marshal(msg)
		if err != nil {
			ctx.Logger().Error("Error marshalling message: %v", err)
			return
		}
		ws.WriteMessage(websocket.TextMessage, msgJSON)
		return
	}

	// User is online ping the callee
	// Publish a message to the callee channel
	msg := messages.NewIncomingCallMessage(callerId, time.Now().Unix())
	msgJSON, err := json.Marshal(msg)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}

	s.Redis.Publish(rdbCtx, calleeChannelID, msgJSON)
}

// TODO: Add a method that "forwards" messages from WS (client 1) -> Redis -> WS (client 2)
// that all it does is serialise the message and publish to the destination user's channel
func rejectCall(ctx echo.Context, s *common.ServerState, rejecterID string, message messages.RejectCallMessage) {
	// Release the dedupe slot so caller can retry without waiting for TTL.
	s.Redis.Del(context.Background(), dedupeCallKey(message.Payload.CallerID, rejecterID))

	// Publish a message to the caller
	payloadJSON, err := json.Marshal(message)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}

	s.Redis.Publish(context.Background(), redisutil.GetUserChannel(message.Payload.CallerID), payloadJSON)
}

func acceptCall(ctx echo.Context, s *common.ServerState, calleeID string, message messages.AcceptCallMessage) {
	// Release the dedupe slot — call moving from "pending" to "in progress".
	s.Redis.Del(context.Background(), dedupeCallKey(message.Payload.CallerID, calleeID))

	// Publish a message to the caller for acceptance
	payloadJSON, err := json.Marshal(message)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}
	s.Redis.Publish(context.Background(), redisutil.GetUserChannel(message.Payload.CallerID), payloadJSON)

	// Next steps after accepting call
	// 1. Create a room with the two participants
	// 2. Create 6 tokens, 2 for each participant per video+data stream, audio stream, and camera stream
	// 3. Send the tokens to the participants
	callerID := message.Payload.CallerID
	caller, err := models.GetUserByID(s.DB, callerID)
	if err != nil {
		ctx.Logger().Error(err)
		sendCommonErrorMessage(s, "Failed to get caller", callerID, calleeID)
		return
	}

	callee, err := models.GetUserByID(s.DB, calleeID)
	if err != nil {
		ctx.Logger().Error(err)
		sendCommonErrorMessage(s, "Failed to get callee", callerID, calleeID)
		return
	}

	roomName := uuid.New().String()
	ctx.Logger().Info("Creating room: ", roomName, " for users ", callerID, " ", calleeID)

	calleeTokens, err := generateLiveKitTokens(s, roomName, callee)
	if err != nil {
		ctx.Logger().Error(err)
		sendCommonErrorMessage(s, "Failed to generate callee tokens", callerID, calleeID)
		return
	}

	callerTokens, err := generateLiveKitTokens(s, roomName, caller)
	if err != nil {
		ctx.Logger().Error(err)
		sendCommonErrorMessage(s, "Failed to generate caller tokens", callerID, calleeID)
		return
	}

	// Publish a message to the caller and the callee
	// with their tokens
	calleeMsg := messages.NewCallTokens(livekitutil.LivekitTokenSet{
		AudioToken:  calleeTokens.AudioToken,
		VideoToken:  calleeTokens.VideoToken,
		CameraToken: calleeTokens.CameraToken,
		Participant: callerID,
	})
	calleeMsgJSON, err := json.Marshal(calleeMsg)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}

	callerMsg := messages.NewCallTokens(livekitutil.LivekitTokenSet{
		AudioToken:  callerTokens.AudioToken,
		VideoToken:  callerTokens.VideoToken,
		CameraToken: callerTokens.CameraToken,
		Participant: calleeID,
	})
	callerMsgJSON, err := json.Marshal(callerMsg)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}

	// Publish the LiveKit tokens to the caller and the callee
	s.Redis.Publish(context.Background(), redisutil.GetUserChannel(message.Payload.CallerID), callerMsgJSON)
	s.Redis.Publish(context.Background(), redisutil.GetUserChannel(calleeID), calleeMsgJSON)

	if s.CallState != nil {
		ctx.Logger().Infof("callstate: AddCallRoom callerID=%s calleeID=%s room=%s", callerID, calleeID, roomName)
		// Ad-hoc 1:1 call with empty display name
		if _, err := s.CallState.AddCallRoom(context.Background(), roomName, []string{callerID, calleeID}, ""); err != nil {
			ctx.Logger().Warnf("callstate.AddCallRoom error: %v", err)
		}
	}

	broadcastPresenceChanged(ctx, s, callerID)

	_ = notifications.SendTelegramNotification(fmt.Sprintf("Call started: %s -> %s", caller.ID, callee.ID), s.Config)
}

func sendCommonErrorMessage(s *common.ServerState, err string, userIDs ...string) {
	for _, userID := range userIDs {
		msg := messages.NewErrorMessage(err)
		msgJSON, err := json.Marshal(msg)
		if err != nil {
			return
		}
		s.Redis.Publish(context.Background(), redisutil.GetUserChannel(userID), msgJSON)
	}
}

func endCall(ctx echo.Context, s *common.ServerState, userID string, message messages.CallEndMessage) {
	// Release the dedupe lock so either party can call again immediately.
	s.Redis.Del(context.Background(), dedupeCallKey(userID, message.Payload.ParticipantID))

	// The peer to notify that the call ended. During ringing this is the only
	// signal the callee gets; for active calls it's recomputed below.
	notifyPeerID := message.Payload.ParticipantID

	if s.CallState != nil {
		ctx.Logger().Infof("callstate: RemoveUser userID=%s", userID)
		roomName, remainingPeers, isNamedRoom, err := s.CallState.RemoveUser(context.Background(), userID)
		if err != nil {
			ctx.Logger().Warnf("callstate.RemoveUser error: %v", err)
		}
		ctx.Logger().Infof("callstate: RemoveUser room=%s remainingPeers=%v isNamedRoom=%v", roomName, remainingPeers, isNamedRoom)

		// Caller is in an active ad-hoc 1:1 room: collapse it and notify the
		// remaining peer (existing behavior).
		if roomName != "" && !isNamedRoom && len(remainingPeers) == 1 {
			s.CallState.RemoveUser(context.Background(), remainingPeers[0])
			notifyPeerID = remainingPeers[0]
		} else if roomName != "" {
			// Named room or >1 peers remaining: do NOT broadcast call_end.
			notifyPeerID = ""
		}
	}

	if notifyPeerID != "" {
		endMsg := messages.NewCallEndMessage(userID)
		endMsgJSON, mErr := json.Marshal(endMsg)
		if mErr == nil {
			s.Redis.Publish(context.Background(), redisutil.GetUserChannel(notifyPeerID), endMsgJSON)
		}
	}

	broadcastPresenceChanged(ctx, s, userID)
}

// handlePresenceAck materializes a candidate room into the snapshot when a
// client confirms it is in the call (any-one ACK validates the whole room).
func handlePresenceAck(ctx echo.Context, s *common.ServerState, userID string, msg messages.PresenceAckMessage) {
	if s.CallState == nil {
		return
	}
	room := msg.Payload.Room
	if !msg.Payload.InCall {
		// Leave the pending guard to expire; the next sweep re-pings if still live.
		ctx.Logger().Debugf("callstate: presence_ack in_call=false userID=%s room=%s", userID, room)
		return
	}

	bgCtx := context.Background()
	members, err := s.CallState.GetPending(bgCtx, room)
	if err != nil {
		ctx.Logger().Warnf("callstate: GetPending error room=%s: %v", room, err)
		return
	}
	// Ensure the acking user is included even if the pending list raced.
	members = append(members, userID)

	changed, err := s.CallState.MaterializeRoom(bgCtx, room, members)
	if err != nil {
		ctx.Logger().Warnf("callstate: MaterializeRoom error room=%s: %v", room, err)
		return
	}
	if err := s.CallState.ClearPending(bgCtx, room); err != nil {
		ctx.Logger().Warnf("callstate: ClearPending error room=%s: %v", room, err)
	}
	if changed {
		ctx.Logger().Infof("callstate: materialized room=%s via ACK from userID=%s", room, userID)
		broadcastPresenceChanged(ctx, s, userID)
	}
}

func broadcastPresenceChanged(ctx echo.Context, s *common.ServerState, userID string) {
	bgCtx := context.Background()

	user, err := models.GetUserByID(s.DB, userID)
	if err != nil {
		ctx.Logger().Warnf("broadcastPresenceChanged: failed to get user %s: %v", userID, err)
		return
	}

	teammates, err := user.GetTeammates(s.DB)
	if err != nil {
		ctx.Logger().Warnf("broadcastPresenceChanged: failed to get teammates for %s: %v", userID, err)
		return
	}

	msg := messages.NewPresenceChangedMessage()
	msgJSON, err := json.Marshal(msg)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}

	s.Redis.Publish(bgCtx, redisutil.GetUserChannel(userID), msgJSON)

	for _, teammate := range teammates {
		channels, chErr := s.Redis.PubSubChannels(bgCtx, redisutil.GetUserChannel(teammate.ID)).Result()
		if chErr != nil {
			ctx.Logger().Warnf("broadcastPresenceChanged: PubSubChannels error for %s: %v", teammate.ID, chErr)
			continue
		}
		if len(channels) > 0 {
			s.Redis.Publish(bgCtx, redisutil.GetUserChannel(teammate.ID), msgJSON)
		}
	}
}

func publishTeammateOnlineMessage(ctx echo.Context, s *common.ServerState, userID, teammateID string) {
	// Ping the teammate that user is online
	msg := messages.NewTeammateOnlineMessage(userID)
	msgJSON, err := json.Marshal(msg)
	if err != nil {
		ctx.Logger().Error(err)
		return
	}

	s.Redis.Publish(context.Background(), redisutil.GetUserChannel(teammateID), msgJSON)
}
