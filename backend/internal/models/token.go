package models

import (
	"time"

	"github.com/google/uuid"
	"gorm.io/gorm"
)

// TokenType represents different types of tokens
type TokenType string

const TokenExpirationDuration = 30 * time.Minute
const TokenTypePasswordReset TokenType = "password_reset"

// Token represents a security token in the system
type Token struct {
	gorm.Model
	UserID    string     `gorm:"not null" json:"user_id" validate:"required"`
	Token     string     `json:"token" gorm:"type:varchar(255);not null;unique;index" validate:"required"`
	TokenType TokenType  `json:"token_type" gorm:"type:varchar(50);not null;index" validate:"required"`
	UsedAt    *time.Time `json:"used_at,omitempty"`
}

func (t *Token) BeforeCreate(tx *gorm.DB) (err error) {
	uuidV7, err := uuid.NewV7()
	if err != nil {
		return err
	}
	t.Token = uuidV7.String()
	return
}

// CreateToken creates a new token record in the database
func (t *Token) CreateToken(db *gorm.DB, tokenType TokenType) error {
	t.CreatedAt = time.Now()
	t.UpdatedAt = time.Now()
	t.TokenType = tokenType
	return db.Create(t).Error
}

// Check if the token is used
func (t *Token) Used() bool {
	return t.UsedAt != nil
}

// IsValid checks if the token is valid
func (t *Token) IsValid() bool {
	expirationTime := t.CreatedAt.Add(TokenExpirationDuration)
	return time.Now().Before(expirationTime)
}
