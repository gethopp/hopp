package handlers

import (
	"net/url"
	"testing"
)

// TestBuildDesktopJoinURL checks the exact deep link shape Slack is given for
// the "Join" button, since the desktop app matches on the path and reads the
// session from the query string.
func TestBuildDesktopJoinURL(t *testing.T) {
	tests := []struct {
		name      string
		sessionID string
		want      string
	}{
		{
			name:      "uuid session id",
			sessionID: "0f0f2a3c-3d4b-4a1e-9c2f-8f7b6a5d4c3b",
			want:      "hopp:///join-session?sessionId=0f0f2a3c-3d4b-4a1e-9c2f-8f7b6a5d4c3b",
		},
		{
			name:      "session id needing query escaping",
			sessionID: "room id&other=1",
			want:      "hopp:///join-session?sessionId=room+id%26other%3D1",
		},
		{
			name:      "empty session id",
			sessionID: "",
			want:      "hopp:///join-session?sessionId=",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := buildDesktopJoinURL(tt.sessionID); got != tt.want {
				t.Errorf("buildDesktopJoinURL(%q) = %q, want %q", tt.sessionID, got, tt.want)
			}
		})
	}
}

// TestBuildDesktopJoinURLParsesBackToSessionID guards the contract the desktop
// app relies on: scheme "hopp", path "/join-session", and a sessionId query
// parameter that round-trips to the original room ID.
func TestBuildDesktopJoinURLParsesBackToSessionID(t *testing.T) {
	const sessionID = "room id&other=1"

	parsed, err := url.Parse(buildDesktopJoinURL(sessionID))
	if err != nil {
		t.Fatalf("url.Parse() returned an error: %v", err)
	}

	if parsed.Scheme != "hopp" {
		t.Errorf("scheme = %q, want %q", parsed.Scheme, "hopp")
	}
	if parsed.Path != "/join-session" {
		t.Errorf("path = %q, want %q", parsed.Path, "/join-session")
	}
	if got := parsed.Query().Get("sessionId"); got != sessionID {
		t.Errorf("sessionId = %q, want %q", got, sessionID)
	}
}
