package models

import (
	"gorm.io/gorm"
)

// Feedback from a user submitted after a call ends
type Feedback struct {
	gorm.Model
	TeamID        string                 `json:"team_id" gorm:"index"`
	RoomID        string                 `json:"room_id" gorm:"index"`
	ParticipantID string                 `json:"participant_id" gorm:"not null;index"`
	Score         int                    `json:"score" gorm:"not null" validate:"required,min=1,max=5"`
	Feedback      string                 `json:"feedback"`                        // Optional text feedback
	Metadata      map[string]interface{} `json:"metadata" gorm:"serializer:json"` // Unstructured metadata for random data we might add later
}
