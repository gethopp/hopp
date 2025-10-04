package models

import (
	"database/sql/driver"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"time"

	"github.com/google/uuid"
	"github.com/stripe/stripe-go/v82"
	"github.com/stripe/stripe-go/v82/subscription"
	"github.com/stripe/stripe-go/v82/subscriptionitem"
	"golang.org/x/crypto/bcrypt"
	"gorm.io/gorm"
)

// EmailSubscriptions tracks user's email subscription preferences
type EmailSubscriptions struct {
	MarketingEmails bool       `gorm:"default:false" json:"marketing_emails"`
	UnsubscribedAt  *time.Time `json:"unsubscribed_at,omitempty"`
}

// If we get more JSON values fields, we can use a Generic
// to avoid copy-paste
func (es EmailSubscriptions) Value() (driver.Value, error) {
	return json.Marshal(es)
}

func (es *EmailSubscriptions) Scan(value interface{}) error {
	b, ok := value.([]byte)
	if !ok {
		return errors.New("type assertion to []byte failed")
	}
	return json.Unmarshal(b, &es)
}

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
	// Email subscription preferences
	EmailSubscriptions EmailSubscriptions `gorm:"type:json" json:"email_subscriptions"`
	// Email unsubscribe token - Different from user ID to avoid bad actors unsubscribing others by their public ID
	UnsubscribeID string `json:"unsubscribe_id" gorm:"unique;not null"`
}

func (u *User) BeforeCreate(tx *gorm.DB) (err error) {
	// Using uuid v7 to be indexable with B-tree
	// Overkill for real
	uuidV7, err := uuid.NewV7()
	if err != nil {
		return err
	}
	u.ID = uuidV7.String()

	// Generate a unique unsubscribe ID
	unsubUUID, err := uuid.NewRandom()
	if err != nil {
		return err
	}
	u.UnsubscribeID = unsubUUID.String()

	u.EmailSubscriptions.MarketingEmails = true
	u.EmailSubscriptions.UnsubscribedAt = nil

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

// UnsubscribeFromAllEmails unsubscribes user from all emails
func (u *User) UnsubscribeFromAllEmails(db *gorm.DB) error {
	now := time.Now()
	u.EmailSubscriptions.UnsubscribedAt = &now
	u.EmailSubscriptions.MarketingEmails = false

	return db.Save(u).Error
}

func updateSubscriptionQuantity(tx *gorm.DB, teamID uint) error {
	teamMembers, err := GetTeamMembersByTeamID(tx, teamID)
	if err != nil {
		return fmt.Errorf("failed to get team members: %w", err)
	}

	newUserCount := len(teamMembers)

	dbSub, err := GetSubscriptionByTeamID(tx, teamID)
	if err != nil {
		return fmt.Errorf("failed to get subscription: %w", err)
	}

	if dbSub == nil || !dbSub.IsActive() {
		// No active subscription found for team, skipping
		return nil
	}

	stripeSubscription, err := subscription.Get(dbSub.StripeSubscriptionID, nil)
	if err != nil {
		return fmt.Errorf("failed to get Stripe subscription: %w", err)
	}

	if len(stripeSubscription.Items.Data) == 0 {
		return fmt.Errorf("no subscription items found")
	}

	subscriptionItemID := stripeSubscription.Items.Data[0].ID

	// Update the subscription item quantity for the next billing cycle
	params := &stripe.SubscriptionItemParams{
		Quantity: stripe.Int64(int64(newUserCount)),
	}

	_, err = subscriptionitem.Update(subscriptionItemID, params)
	if err != nil {
		return fmt.Errorf("failed to update subscription quantity: %w", err)
	}

	return nil
}

func (u *User) AfterCreate(tx *gorm.DB) (err error) {
	_ = updateSubscriptionQuantity(tx, *u.TeamID)
	return nil
}

func (u *User) AfterDelete(tx *gorm.DB) (err error) {
	_ = updateSubscriptionQuantity(tx, *u.TeamID)
	return nil
}

func GetAdminUserForTeam(db *gorm.DB, teamID uint) (*User, error) {
	var adminUser User
	result := db.Where("team_id = ? AND is_admin = ?", teamID, true).First(&adminUser)
	if result.Error != nil {
		return nil, result.Error
	}
	return &adminUser, nil
}

type UserWithSubscription struct {
	User
	IsPro       bool       `json:"is_pro"`
	IsTrial     bool       `json:"is_trial"`
	TrialEndsAt *time.Time `json:"trial_ends_at,omitempty"`
}

// GetUserWithSubscription returns a user with subscription information
// 1. Check if team is manually upgraded, if so return true to `is_pro`
// 2. Fetch if any sub for their team exists and if its active
// 3. If no sub, return `IsTrial` and `TrialEndsAt`
func GetUserWithSubscription(db *gorm.DB, user *User) (*UserWithSubscription, error) {
	team, err := GetTeamByID(db, strconv.Itoa(int(*user.TeamID)))
	if err != nil {
		return nil, err
	}

	if team.IsManualUpgrade {
		return &UserWithSubscription{User: *user, IsPro: true}, nil
	}

	sub, err := GetSubscriptionByTeamID(db, *user.TeamID)
	if err != nil {
		return nil, err
	}

	if sub != nil && sub.IsActive() {
		return &UserWithSubscription{User: *user, IsPro: true}, nil
	}

	// Trial will be team creation + 30 days
	trialEndsAt := team.CreatedAt.AddDate(0, 0, 30)
	return &UserWithSubscription{
		User:        *user,
		IsTrial:     true,
		TrialEndsAt: &trialEndsAt,
		IsPro:       false,
	}, nil
}
