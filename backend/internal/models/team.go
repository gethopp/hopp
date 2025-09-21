package models

import (
	"errors"

	"gorm.io/gorm"
)

type Team struct {
	gorm.Model
	Name            string `gorm:"not null" json:"name" validate:"required"`
	IsManualUpgrade bool   `gorm:"default:false" json:"is_manual_upgrade"`
}

func GetTeamByID(db *gorm.DB, id string) (*Team, error) {
	var team Team
	result := db.Where("id = ?", id).First(&team)

	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, errors.New("Team not found")
		}
		return nil, result.Error
	}
	return &team, nil
}

func GetTeamMembersByTeamID(db *gorm.DB, teamID uint) ([]User, error) {
	var users []User
	result := db.Where("team_id = ?", teamID).Find(&users)
	return users, result.Error
}

func GetTeamWithSubscription(db *gorm.DB, id string) (*Team, error) {
	var team Team
	result := db.Preload("Subscription").Where("id = ?", id).First(&team)

	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, errors.New("Team not found")
		}
		return nil, result.Error
	}
	return &team, nil
}

// TeamInvitation is a misc model to store team invitation URLs
// It will have an expiry date from its creation date of 2 days.
// This is to prevent abuse of the invitation system.
type TeamInvitation struct {
	gorm.Model
	TeamID   int `gorm:"not null" json:"team_id" validate:"required"`
	Team     Team
	UniqueID string `gorm:"not null" json:"unique_id" validate:"required"`
}
