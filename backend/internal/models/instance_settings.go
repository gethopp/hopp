package models

import (
	"errors"

	"gorm.io/gorm"
	"gorm.io/gorm/clause"
)

// instanceSettingsID is the primary key of the singleton settings row.
const instanceSettingsID = 1

// InstanceSettings holds server-wide configuration that is managed from the
// web UI. There is exactly one row. Every field defaults to the behaviour of
// installations that predate the setting, so adding a field never changes how
// an existing instance behaves.
type InstanceSettings struct {
	ID uint `gorm:"primaryKey" json:"-"`
	// SignupRequiresInvite, when true, rejects sign-ups that do not carry a
	// valid team invite. The first account on an empty instance is exempt so a
	// fresh install can be bootstrapped.
	SignupRequiresInvite bool `gorm:"not null;default:false" json:"signup_requires_invite"`
}

// GetInstanceSettings returns the singleton settings row, creating it with
// defaults on first use.
func GetInstanceSettings(db *gorm.DB) (*InstanceSettings, error) {
	settings := InstanceSettings{ID: instanceSettingsID}
	// DoNothing keeps concurrent first requests from failing on the primary key.
	if err := db.Clauses(clause.OnConflict{DoNothing: true}).Create(&settings).Error; err != nil {
		return nil, err
	}
	if err := db.First(&settings, instanceSettingsID).Error; err != nil {
		return nil, err
	}
	return &settings, nil
}

// SetSignupRequiresInvite persists the sign-up invite requirement.
func SetSignupRequiresInvite(db *gorm.DB, required bool) (*InstanceSettings, error) {
	if _, err := GetInstanceSettings(db); err != nil {
		return nil, err
	}
	if err := db.Model(&InstanceSettings{}).
		Where("id = ?", instanceSettingsID).
		Update("signup_requires_invite", required).Error; err != nil {
		return nil, err
	}
	return GetInstanceSettings(db)
}

// GetInstanceOwnerID returns the ID of the instance owner: the earliest
// account that still exists. Hopp has no instance-wide admin role (is_admin is
// scoped to a user's own team), so ownership is positional. It returns "" when
// the instance has no users.
func GetInstanceOwnerID(db *gorm.DB) (string, error) {
	var owner User
	err := db.Select("id").Order("created_at ASC, id ASC").First(&owner).Error
	if errors.Is(err, gorm.ErrRecordNotFound) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	return owner.ID, nil
}
