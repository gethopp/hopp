package models

import (
	"time"

	"github.com/google/uuid"
	"gorm.io/gorm"
)

type Room struct {
	ID        string    `json:"id" gorm:"unique;not null"` // Standard field for the primary key
	Name      string    `gorm:"not null" json:"name" validate:"required"`
	UserID    string    `gorm:"not null" validate:"required"`
	CreatedAt time.Time `json:"created_at"` // Automatically managed by GORM for creation time
	UpdatedAt time.Time `json:"updated_at"` // Automatically managed by GORM for update time
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
