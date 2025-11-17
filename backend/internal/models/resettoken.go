package models

import (
	"time"

	"gorm.io/gorm"
)

// ResetTokenExpirationDuration is the duration for which a reset token remains valid
const ResetTokenExpirationDuration = 30 * time.Minute

// ResetToken represents a password reset token in the system
type ResetToken struct {
	gorm.Model
	UserID string     `gorm:"not null" json:"user_id" validate:"required"`
	Token  string     `json:"token" gorm:"type:uuid;not null;unique;index;default:gen_random_uuid()" validate:"required"`
	UsedAt *time.Time `json:"used_at,omitempty"`
}

// CreateResetToken creates a new reset token record in the database
func (t *ResetToken) CreateResetToken(db *gorm.DB) error {
	t.CreatedAt = time.Now()
	t.UpdatedAt = time.Now()
	return db.Create(t).Error
}

// Used checks if the token is used
func (t *ResetToken) Used() bool {
	return t.UsedAt != nil
}

// IsValid checks if the token is valid
func (t *ResetToken) IsValid() bool {
	expirationTime := t.CreatedAt.Add(ResetTokenExpirationDuration)
	return time.Now().Before(expirationTime)
}
