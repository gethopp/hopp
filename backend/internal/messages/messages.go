package messages

import (
	"encoding/json"
	"fmt"
	"hopp-backend/internal/common"
)

// MessageType represents the type of WebSocket message
type MessageType string

// Define constants for message types to prevent typos and provide better IDE support
const (
	// Server -> Client: Success message when websocket connection is established
	MessageTypeSuccess MessageType = "success"
	// Client -> Server: Call request from caller to callee (with callee id)
	MessageTypeCallRequest MessageType = "call_request"
	// Server -> Client: Call request from caller (with caller id)
	MessageTypeIncomingCall MessageType = "incoming_call"
	// Server -> Client: Callee is offline
	MessageTypeCalleeOffline MessageType = "callee_offline"
	// Client -> Server: Reject call request (caller id)
	MessageTypeCallReject MessageType = "call_reject"
	// Client -> Server: Accept call request (caller id)
	MessageTypeCallAccept MessageType = "call_accept"
	// Server -> Cleints: New call tokens
	MessageTypeNewCallTokens MessageType = "call_tokens"

	// Server -> Client: Error message when something goes wrong
	MessageTypeError MessageType = "error"

	MessageTypeCallEnd MessageType = "call_end"
	// Client -> Server: Ping message
	MessageTypePing MessageType = "ping"
	// Server -> Client: Pong message
	MessageTypePong MessageType = "pong"

	// Client -> Server and Server -> Client: User has become online
	MessageTypeTeammateOnline MessageType = "teammate_online"
)

// BaseMessage represents the common structure of all WebSocket messages
type BaseMessage struct {
	Type MessageType `json:"type" validate:"required"`
	// Using RawMessage to delay JSON parsing until we know the correct type
	RawPayload json.RawMessage `json:"payload"`
}

// SuccessPayload represents the payload for success messages
type SuccessPayload struct {
	Message string `json:"message"`
}

// SuccessMessage is a complete success message
type SuccessMessage struct {
	Type    MessageType    `json:"type"`
	Payload SuccessPayload `json:"payload"`
}

// CallRequestPayload represents the payload for call request messages
type CallRequestPayload struct {
	CalleeID string `json:"callee_id" validate:"required"`
}

// CallRequestMessage is a complete call request message
type CallRequestMessage struct {
	Type    MessageType        `json:"type"`
	Payload CallRequestPayload `json:"payload"`
}

// CallEndPayload represents the payload for call end messages
type CallEndPayload struct {
	ParticipantID string `json:"participant_id" validate:"required"`
}

// CallEndMessage is a complete call end message
type CallEndMessage struct {
	Type    MessageType    `json:"type"`
	Payload CallEndPayload `json:"payload"`
}

// IncomingCallPayload represents the payload for an incoming call by another user
type IncomingCallPayload struct {
	CallerID string `json:"caller_id" validate:"required"`
}

// IncomingCallMessage is a complete call request message
type IncomingCallMessage struct {
	Type    MessageType         `json:"type"`
	Payload IncomingCallPayload `json:"payload"`
}

// AcceptCallMessage is the message to accept a call request
type AcceptCallMessage struct {
	Type    MessageType `json:"type"`
	Payload struct {
		CallerID string `json:"caller_id" validate:"required"`
	} `json:"payload"`
}

// CallTokensMessage sends to both users the livekit tokens to start the call
type CallTokensMessage struct {
	Type    MessageType `json:"type"`
	Payload struct {
		common.LivekitTokenSet
	} `json:"payload"`
}

// RejectCallMessage is the message to reject a call request
type RejectCallMessage struct {
	Type    MessageType `json:"type"`
	Payload struct {
		CallerID     string `json:"caller_id" validate:"required"`
		RejectReason string `json:"reject_reason,omitempty" validate:"omitempty,oneof=in-call rejected"`
	} `json:"payload"`
}

type ErrorPayload struct {
	Error string `json:"error" validate:"required"`
}

// ErrorMessage is a complete error message when something
// is not as expected or an error occurs and needs to be sent to the client
type ErrorMessage struct {
	Type    MessageType  `json:"type"`
	Payload ErrorPayload `json:"payload"`
}

// PingPayload represents the payload for ping messages
type PingPayload struct {
	Message string `json:"message"`
}

// PingMessage is a simple ping message with just the type
type PingMessage struct {
	Type    MessageType `json:"type"`
	Payload PingPayload `json:"payload"`
}

// PongPayload represents the payload for pong messages
type PongPayload struct {
	Message string `json:"message"`
}

// PongMessage is a complete pong message
type PongMessage struct {
	Type    MessageType `json:"type"`
	Payload PongPayload `json:"payload"`
}

// CalleeOfflinePayload represents the payload for callee offline messages
type CalleeOfflinePayload struct {
	CalleeID string `json:"callee_id"`
}

// CalleeOfflineMessage is a complete callee offline message
type CalleeOfflineMessage struct {
	Type    MessageType          `json:"type"`
	Payload CalleeOfflinePayload `json:"payload"`
}

// UserOnlinePayload represents the payload for user online messages
type TeammateOnlinePayload struct {
	TeammateID string `json:"teammate_id"`
}

// UserOnlineMessage is the message to notify that a user has come online
type TeammateOnlineMessage struct {
	Type    MessageType           `json:"type"`
	Payload TeammateOnlinePayload `json:"payload"`
}

// NewCalleeOfflineMessage creates a new callee offline message
func NewCalleeOfflineMessage(calleeID string) *CalleeOfflineMessage {
	return &CalleeOfflineMessage{
		Type: MessageTypeCalleeOffline,
		Payload: CalleeOfflinePayload{
			CalleeID: calleeID,
		},
	}
}

// ParsedMessage is a union type of all possible message types
type ParsedMessage struct {
	Success               *SuccessMessage
	Pong                  *PongMessage
	Ping                  *PingMessage
	CallRequest           *CallRequestMessage
	CallEnd               *CallEndMessage
	CalleeOffline         *CalleeOfflineMessage
	IncomingCall          *IncomingCallMessage
	AcceptCallMessage     *AcceptCallMessage
	RejectCallMessage     *RejectCallMessage
	CallTokensMessage     *CallTokensMessage
	TeammateOnlineMessage *TeammateOnlineMessage
	Error                 *ErrorMessage
}

// ParseMessage parses a raw message into a ParsedMessage
func ParseMessage(data []byte) (*ParsedMessage, error) {
	var base BaseMessage
	if err := json.Unmarshal(data, &base); err != nil {
		return nil, fmt.Errorf("failed to parse base message: %w", err)
	}

	parsed := &ParsedMessage{}

	switch base.Type {
	case MessageTypeCallRequest:
		var msg CallRequestMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.CallRequest = &msg
	case MessageTypeIncomingCall:
		var msg IncomingCallMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.IncomingCall = &msg
	case MessageTypeCalleeOffline:
		var msg CalleeOfflineMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.CalleeOffline = &msg
	case MessageTypeCallReject:
		var msg RejectCallMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.RejectCallMessage = &msg
	case MessageTypeCallAccept:
		var msg AcceptCallMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.AcceptCallMessage = &msg
	case MessageTypeNewCallTokens:
		var msg CallTokensMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.CallTokensMessage = &msg
	case MessageTypeCallEnd:
		var msg CallEndMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.CallEnd = &msg
	case MessageTypePing:
		var msg PingMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.Ping = &msg
	case MessageTypeTeammateOnline:
		var msg TeammateOnlineMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			return nil, err
		}
		parsed.TeammateOnlineMessage = &msg
	}

	return parsed, nil
}

// Helper functions to create typed messages

// NewSuccessMessage creates a new success message
func NewSuccessMessage(message string) SuccessMessage {
	return SuccessMessage{
		Type: MessageTypeSuccess,
		Payload: SuccessPayload{
			Message: message,
		},
	}
}

// NewCallRequestMessage creates a new call request message
func NewCallRequestMessage(calleeID string) CallRequestMessage {
	return CallRequestMessage{
		Type: MessageTypeCallRequest,
		Payload: CallRequestPayload{
			CalleeID: calleeID,
		},
	}
}

// NewCallEndMessage creates a new call end message
func NewCallEndMessage(callID string) CallEndMessage {
	return CallEndMessage{
		Type: MessageTypeCallEnd,
		Payload: CallEndPayload{
			ParticipantID: callID,
		},
	}
}

func NewIncomingCallMessage(callerID string) IncomingCallMessage {
	return IncomingCallMessage{
		Type: MessageTypeIncomingCall,
		Payload: IncomingCallPayload{
			CallerID: callerID,
		},
	}
}

func NewErrorMessage(err string) ErrorMessage {
	return ErrorMessage{
		Type: MessageTypeError,
		Payload: ErrorPayload{
			Error: err,
		},
	}
}

func NewCallTokens(tokens common.LivekitTokenSet) CallTokensMessage {
	return CallTokensMessage{
		Type: MessageTypeNewCallTokens,
		Payload: struct{ common.LivekitTokenSet }{
			LivekitTokenSet: tokens,
		},
	}
}

// NewPongMessage creates a new pong message
func NewPongMessage() PongMessage {
	return PongMessage{
		Type: MessageTypePong,
		Payload: PongPayload{
			Message: "pong",
		},
	}
}

// MessageHandler defines a type for message handling functions
type MessageHandler func(message *ParsedMessage) error

// NewTeammateOnlineMessage creates a new teammate online message
func NewTeammateOnlineMessage(teammateID string) TeammateOnlineMessage {
	return TeammateOnlineMessage{
		Type: MessageTypeTeammateOnline,
		Payload: TeammateOnlinePayload{
			TeammateID: teammateID,
		},
	}
}
