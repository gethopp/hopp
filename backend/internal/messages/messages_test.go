package messages

import (
	"encoding/json"
	"testing"
)

func TestTeammateOnlineMessageWithInfo(t *testing.T) {
	msg := NewTeammateOnlineMessageWithInfo(
		"user-123",
		"Grace",
		"Hopper",
		"grace@example.com",
		"https://example.com/avatar.png",
	)

	data, err := json.Marshal(msg)
	if err != nil {
		t.Fatalf("marshal teammate online message: %v", err)
	}

	parsed, err := ParseMessage(data)
	if err != nil {
		t.Fatalf("parse teammate online message: %v", err)
	}

	payload := parsed.TeammateOnlineMessage.Payload
	if payload.TeammateID != "user-123" {
		t.Fatalf("expected teammate_id user-123, got %q", payload.TeammateID)
	}
	if payload.FirstName != "Grace" || payload.LastName != "Hopper" {
		t.Fatalf("expected teammate name in payload, got %q %q", payload.FirstName, payload.LastName)
	}
	if payload.Email != "grace@example.com" {
		t.Fatalf("expected email in payload, got %q", payload.Email)
	}
	if payload.AvatarURL != "https://example.com/avatar.png" {
		t.Fatalf("expected avatar_url in payload, got %q", payload.AvatarURL)
	}
}

func TestTeammateOnlineMessageParsesLegacyPayload(t *testing.T) {
	data := []byte(`{"type":"teammate_online","payload":{"teammate_id":"user-123"}}`)

	parsed, err := ParseMessage(data)
	if err != nil {
		t.Fatalf("parse legacy teammate online message: %v", err)
	}

	payload := parsed.TeammateOnlineMessage.Payload
	if payload.TeammateID != "user-123" {
		t.Fatalf("expected teammate_id user-123, got %q", payload.TeammateID)
	}
	if payload.FirstName != "" || payload.LastName != "" || payload.Email != "" || payload.AvatarURL != "" {
		t.Fatalf("expected empty optional user info for legacy payload, got %+v", payload)
	}
}
