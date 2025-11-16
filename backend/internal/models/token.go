package models

import (
	"time"

	"gorm.io/gorm"
)

// TokenType represents different types of tokens
type TokenType string

const (
	TokenTypePasswordReset TokenType = "password_reset"
)

// Token represents a security token in the system
type Token struct {
	gorm.Model
	UserID    string     `gorm:"not null" json:"user_id" validate:"required"`
	Token     string     `json:"token" gorm:"type:varchar(255);not null;unique;index" validate:"required"`
	TokenType TokenType  `json:"token_type" gorm:"type:varchar(50);not null;index" validate:"required"`
	IsUsed    bool       `json:"is_used" gorm:"default:false;not null"`
	UsedAt    *time.Time `json:"used_at,omitempty"`
}

// CreateToken creates a new token record in the database
func (t *Token) CreateToken(db *gorm.DB, tokenType TokenType, token string) error {
	t.CreatedAt = time.Now()
	t.UpdatedAt = time.Now()
	t.TokenType = tokenType
	t.Token = token
	return db.Create(t).Error
}
