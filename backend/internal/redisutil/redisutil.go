// Package redisutil holds Redis naming conventions shared across packages.
package redisutil

import "fmt"

// GetUserChannel returns the Redis pub/sub channel name for a given user ID.
func GetUserChannel(userID string) string {
	return fmt.Sprintf("channel-user-%s", userID)
}
