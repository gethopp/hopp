// Package livekitutil holds small, dependency-free helpers for working with
// LiveKit URLs and participant identities. It lives in its own leaf package so
// both handlers and callstate can share it without an import cycle.
package livekitutil

import (
	"fmt"
	"net/url"
	"strings"
)

// LivekitTokenSet holds the per-track tokens issued for a LiveKit room.
type LivekitTokenSet struct {
	AudioToken  string `json:"audioToken"`
	VideoToken  string `json:"videoToken"`
	CameraToken string `json:"cameraToken"`
	Participant string `json:"participant"`
}

// ConvertURLToHTTP converts a LiveKit WebSocket URL (wss:// or ws://) to an HTTP
// URL. Uses net/url for safe URL parsing instead of string concatenation.
func ConvertURLToHTTP(livekitURL string) (string, error) {
	parsed, err := url.Parse(livekitURL)
	if err != nil {
		return "", fmt.Errorf("failed to parse LiveKit URL: %w", err)
	}

	// Convert scheme: wss -> https, ws -> http
	switch parsed.Scheme {
	case "wss":
		parsed.Scheme = "https"
	case "ws":
		parsed.Scheme = "http"
	case "https", "http":
		// Already HTTP(S), no change needed
	default:
		return "", fmt.Errorf("unexpected LiveKit URL scheme: %s", parsed.Scheme)
	}

	return parsed.String(), nil
}

// ExtractUserIDFromIdentity extracts the user ID from a LiveKit participant
// identity. Identity format is: "room:<roomId>:<userId>:<trackType>"
// (e.g. "room:abc123:usr_xyz:audio"). Returns an error if the identity doesn't
// match the expected format.
func ExtractUserIDFromIdentity(identity string) (string, error) {
	parts := strings.Split(identity, ":")
	if len(parts) >= 4 && parts[0] == "room" {
		return parts[2], nil
	}
	return "", fmt.Errorf("invalid identity format: %s", identity)
}
