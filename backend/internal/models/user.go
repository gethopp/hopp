package models

import (
	"errors"
	"fmt"
	"hopp-backend/internal/utils"
	"time"

	"github.com/google/uuid"
	"golang.org/x/crypto/bcrypt"
	"gorm.io/gorm"
)

type User struct {
	ID             string    `json:"id" gorm:"unique;not null"` // Standard field for the primary key
	FirstName      string    `gorm:"not null" json:"first_name" validate:"required"`
	LastName       string    `gorm:"not null" json:"last_name" validate:"required"`
	Email          string    `gorm:"not null;unique" json:"email" validate:"required,email"`
	IsAdmin        bool      `gorm:"default:false" json:"is_admin"`
	TeamID         *uint     `json:"team_id" gorm:"default:null"`
	Team           *Team     `json:"team,omitempty"`
	Password       string    `gorm:"-" json:"password" validate:"required,min=8"`
	HashedPassword string    `json:"-"` // Removed "not null" constraint
	AvatarURL      string    `json:"avatar_url"`
	CreatedAt      time.Time `json:"created_at"` // Automatically managed by GORM for creation time
	UpdatedAt      time.Time `json:"updated_at"` // Automatically managed by GORM for update time
	// Can keep data like Slack workspace friends etc
	SocialMetadata map[string]interface{} `gorm:"serializer:json" json:"social_metadata,omitempty"`
	// General user metadata for onboarding, preferences, etc.
	Metadata map[string]interface{} `gorm:"serializer:json" json:"metadata"`
}

func (u *User) BeforeCreate(tx *gorm.DB) (err error) {
	// Using uuid v7 to be indexable with B-tree
	// Overkill for real
	uuidV7, err := uuid.NewV7()
	if err != nil {
		return err
	}
	u.ID = uuidV7.String()

	// Hash password if it's set
	if u.Password != "" {
		hashedPassword, err := bcrypt.GenerateFromPassword([]byte(u.Password), bcrypt.DefaultCost)
		if err != nil {
			return err
		}
		u.HashedPassword = string(hashedPassword)
		// Clear the plain text password
		u.Password = ""
	}

	return
}

func (u *User) CheckPassword(password string) bool {
	err := bcrypt.CompareHashAndPassword([]byte(u.HashedPassword), []byte(password))
	return err == nil
}

func GetUserByEmail(db *gorm.DB, email string) (*User, error) {
	var user User
	result := db.Where("email = ?", email).First(&user)

	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, errors.New("User not found")
		}
		return nil, result.Error
	}
	return &user, nil
}

func GetUserByID(db *gorm.DB, id string) (*User, error) {
	var user *User
	result := db.Where("id = ?", id).First(&user)

	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, errors.New("User not found")
		}
		return nil, result.Error
	}
	return user, nil
}

func (u *User) GetRedisChannel() string {
	return fmt.Sprintf("channel-user-%s", u.ID)
}

type UserWithActivity struct {
	User
	IsActive bool `json:"is_active"`
}

func (u *User) GetTeammates(db *gorm.DB) ([]UserWithActivity, error) {
	// First preload the user's team
	if err := db.Preload("Team").Where("id = ?", u.ID).First(u).Error; err != nil {
		return nil, err
	}

	if u.Team == nil {
		return []UserWithActivity{}, nil
	}

	var teammates []User
	if err := db.Select("id, first_name, last_name, email, avatar_url, team_id, is_admin, created_at, updated_at").
		Where("team_id = ? AND id != ?", u.TeamID, u.ID).
		Find(&teammates).Error; err != nil {
		return nil, err
	}

	// Convert to UserWithActivity
	teammatesWithActivity := make([]UserWithActivity, len(teammates))
	for i, teammate := range teammates {
		teammatesWithActivity[i] = UserWithActivity{
			User:     teammate,
			IsActive: false, // Will be set by the handler
		}
	}

	return teammatesWithActivity, nil
}

// GetDisplayName returns the user's display name
func (u *User) GetDisplayName() string {
	if u.LastName == "" {
		return u.FirstName
	}
	return fmt.Sprintf("%s %s", u.FirstName, u.LastName)
}
