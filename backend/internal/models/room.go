package models

import (
	"encoding/json"
	"time"

	"github.com/google/uuid"
	"gorm.io/datatypes"
	"gorm.io/gorm"
)

// RoomType defines the type/origin of a room.
type RoomType string

const (
	// RoomTypeDefault is a standard room created from the desktop app.
	RoomTypeDefault RoomType = "default"
	// RoomTypeSlack is a room created via Slack /hopp command.
	RoomTypeSlack RoomType = "slack"
	// RoomTypeTeams is a room created via Microsoft Teams (future).
	// RoomTypeTeams RoomType = "teams"
)

// Room represents a Hopp room where users can pair/collaborate.
type Room struct {
	ID        string    `json:"id" gorm:"unique;not null"` // Standard field for the primary key
	Name      string    `gorm:"not null" json:"name" validate:"required"`
	UserID    string    `gorm:"not null" json:"user_id" validate:"required"`
	CreatedAt time.Time `json:"created_at"` // Automatically managed by GORM for creation time
	UpdatedAt time.Time `json:"updated_at"` // Automatically managed by GORM for update time
	TeamID    *uint     `json:"team_id" gorm:"default:null"`
	Team      *Team     `json:"team,omitempty"`

	// Type indicates the origin/type of the room (default, slack, teams, etc.)
	// Used to determine special handling (e.g., Slack Call API updates).
	Type RoomType `json:"type" gorm:"default:'default';not null"`

	// Temp indicates if this is a temporary room.
	// Temporary rooms are NOT listed in the /rooms endpoint but can be joined.
	// Rooms created from Slack are temporary by default.
	Temp bool `json:"temp" gorm:"default:false;not null"`

	// Metadata stores integration-specific data as JSON.
	// This allows flexibility for different room types without schema changes.
	//
	// For Slack rooms (Type="slack"), expected structure:
	//   {
	//     "slack_team_id": "T1234567890",      // Slack workspace ID
	//     "slack_channel_id": "C1234567890",   // Channel where /hopp was invoked
	//     "slack_call_id": "R1234567890",      // Slack Call ID for calls.participants.add/remove
	//     "slack_message_ts": "1234567890.123" // Message timestamp (may be same as call_id)
	//   }
	//
	Metadata datatypes.JSON `json:"metadata,omitempty" gorm:"type:jsonb"`

	// LastParticipantLeftAt tracks when the last participant left the room.
	// Used by the cleanup goroutine to determine if the room should be deleted.
	// nil means there are still participants or the room was just created.
	LastParticipantLeftAt *time.Time `json:"last_participant_left_at,omitempty"`
}

func (r *Room) BeforeCreate(tx *gorm.DB) (err error) {
	// Using uuid v7 to be indexable with B-tree
	// Overkill for real
	uuidV7, err := uuid.NewV7()
	if err != nil {
		return err
	}
	r.ID = uuidV7.String()

	return
}

// SlackMetadata represents the metadata structure for Slack rooms.
type SlackMetadata struct {
	SlackTeamID    string `json:"slack_team_id"`
	SlackChannelID string `json:"slack_channel_id"`
	SlackCallID    string `json:"slack_call_id"`
	SlackMessageTS string `json:"slack_message_ts"`
}

// GetSlackMetadata extracts and parses SlackMetadata from the room's Metadata field.
// Returns nil if the room is not a Slack room or if metadata is empty.
func (r *Room) GetSlackMetadata() *SlackMetadata {
	if r.Type != RoomTypeSlack || r.Metadata == nil {
		return nil
	}

	var meta SlackMetadata
	if err := json.Unmarshal(r.Metadata, &meta); err != nil {
		return nil
	}
	return &meta
}

// SetSlackMetadata sets the SlackMetadata on the room's Metadata field.
func (r *Room) SetSlackMetadata(meta *SlackMetadata) error {
	data, err := json.Marshal(meta)
	if err != nil {
		return err
	}
	r.Metadata = datatypes.JSON(data)
	return nil
}

// GetRoomByID retrieves a room by its ID.
func GetRoomByID(db *gorm.DB, roomID string) (*Room, error) {
	var room Room
	result := db.Where("id = ?", roomID).First(&room)
	if result.Error != nil {
		if result.Error == gorm.ErrRecordNotFound {
			return nil, nil
		}
		return nil, result.Error
	}
	return &room, nil
}
