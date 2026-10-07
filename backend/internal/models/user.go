package models

import (
	"database/sql/driver"
	"encoding/json"
	"errors"
	"fmt"
	"hopp-backend/internal/redisutil"
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
	MarketingEmails  bool       `gorm:"default:false" json:"marketing_emails"`
	LinuxWaitingList bool       `gorm:"default:false" json:"linux_waitinglist"`
	UnsubscribedAt   *time.Time `json:"unsubscribed_at,omitempty"`
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

type UserProfile struct {
	ID        string    `json:"id" gorm:"unique;not null"` // Standard field for the primary key
	FirstName string    `gorm:"not null" json:"first_name" validate:"required"`
	LastName  string    `gorm:"not null" json:"last_name" validate:"required"`
	Email     string    `gorm:"not null;unique" json:"email" validate:"required,email"`
	IsAdmin   bool      `gorm:"default:false" json:"is_admin"`
	TeamID    *uint     `json:"team_id" gorm:"default:null"`
	AvatarURL string    `json:"avatar_url"`
	CreatedAt time.Time `json:"created_at"` // Automatically managed by GORM for creation time
	UpdatedAt time.Time `json:"updated_at"` // Automatically managed by GORM for update time
}

type User struct {
	UserProfile `gorm:"embedded"`

	Team           *Team  `json:"team,omitempty"`
	Password       string `gorm:"-" json:"password" validate:"required,min=8"`
	HashedPassword string `json:"-"` // Removed "not null" constraint
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
	u.EmailSubscriptions.LinuxWaitingList = false
	u.EmailSubscriptions.UnsubscribedAt = nil

	// Hash password if it's set
	if u.Password != "" {
		hashedPassword, err := HashPassword(u.Password)
		if err != nil {
			return err
		}
		u.HashedPassword = hashedPassword
		// Clear the plain text password
		u.Password = ""
	}

	return
}

func HashPassword(password string) (string, error) {
	hashedPassword, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return "", err
	}
	return string(hashedPassword), nil
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

// GetUserProfileByEmail fetches only the UserProfile columns for a user. It is
// the slim variant used on hot, read-only paths (auth middleware, websocket
// handshake, teammate/presence polling) to avoid shipping the wide `users` row
// (hashed_password, social_metadata, metadata, email_subscriptions, ...) on
// every request.
func GetUserProfileByEmail(db *gorm.DB, email string) (*UserProfile, error) {
	var profile UserProfile
	result := db.Model(&User{}).Where("email = ?", email).First(&profile)

	if result.Error != nil {
		if errors.Is(result.Error, gorm.ErrRecordNotFound) {
			return nil, errors.New("User not found")
		}
		return nil, result.Error
	}
	return &profile, nil
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

func (u *UserProfile) GetRedisChannel() string {
	return redisutil.GetUserChannel(u.ID)
}

type UserWithActivity struct {
	UserProfile
	IsActive bool `json:"is_active"`
}

func (u *UserProfile) GetTeammates(db *gorm.DB) ([]UserWithActivity, error) {
	if u.TeamID == nil {
		return []UserWithActivity{}, nil
	}

	var teammates []UserProfile
	if err := db.Model(&User{}).
		Where("team_id = ? AND id != ?", *u.TeamID, u.ID).
		Find(&teammates).Error; err != nil {
		return nil, err
	}

	// Convert to UserWithActivity
	teammatesWithActivity := make([]UserWithActivity, len(teammates))
	for i, teammate := range teammates {
		teammatesWithActivity[i] = UserWithActivity{
			UserProfile: teammate,
			IsActive:    false, // Will be set by the handler
		}
	}

	return teammatesWithActivity, nil
}

// GetDisplayName returns the user's display name
func (u *UserProfile) GetDisplayName() string {
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

func UpdateSubscriptionQuantity(tx *gorm.DB, teamID uint) error {
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

	item := stripeSubscription.Items.Data[0]
	oldQty := item.Quantity
	newQty := int64(newUserCount)

	if newQty == oldQty {
		return nil
	}

	params := &stripe.SubscriptionItemParams{
		Quantity: stripe.Int64(newQty),
	}

	// Yearly seat additions get an immediate prorated invoice so teams can't
	// ride free seats until renewal. Monthly subs and yearly seat removals
	// keep the Stripe default (create_prorations → credit/debit on next invoice).
	if dbSub.BillingInterval == IntervalYearly && newQty > oldQty {
		params.ProrationBehavior = stripe.String("always_invoice")
	}

	_, err = subscriptionitem.Update(item.ID, params)
	if err != nil {
		return fmt.Errorf("failed to update subscription quantity: %w", err)
	}

	return nil
}

func (u *User) AfterCreate(tx *gorm.DB) (err error) {
	_ = UpdateSubscriptionQuantity(tx, *u.TeamID)
	return nil
}

func (u *User) AfterDelete(tx *gorm.DB) (err error) {
	_ = UpdateSubscriptionQuantity(tx, *u.TeamID)
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

type TeamAccess struct {
	IsPro       bool       `json:"is_pro"`
	IsTrial     bool       `json:"is_trial"`
	TrialEndsAt *time.Time `json:"trial_ends_at,omitempty"`
}

// Active reports whether the team is Pro or still within its trial window.
func (a TeamAccess) Active(now time.Time) bool {
	return a.IsPro || (a.IsTrial && a.TrialEndsAt != nil && a.TrialEndsAt.After(now))
}

type UserWithSubscription struct {
	User
	TeamAccess
}

// hardPaywallCutoff is the launch date of the card-required trial. Teams created
// before this date keep the legacy free trial derived from team.CreatedAt; teams
// created on or after it get no free trial and must have an active/trialing
// Stripe subscription (a card-on-file trial) to access the product for the first time
// (they can still cancel anytime, and then will see a limited dashboard).
var hardPaywallCutoff = time.Date(2026, time.July, 10, 16, 30, 0, 0, time.UTC)

// IsTeamPostCutoff reports whether a team is subject to the hard paywall (created
// on or after the cutoff date).
func IsTeamPostCutoff(team *Team) bool {
	return !team.CreatedAt.Before(hardPaywallCutoff)
}

// GetTeamAccess computes a team's access state (pro/trial).
// When stripeEnabled is false (self-hosted deployments without Stripe), the team
// is always treated as Pro.
// 1. Check if team is manually upgraded, if so return pro
// 2. Fetch if any sub for the team exists and is active
// 3. If no sub, return trial state derived from the team creation date
func GetTeamAccess(db *gorm.DB, teamID uint, stripeEnabled bool) (TeamAccess, error) {
	if !stripeEnabled {
		return TeamAccess{IsPro: true}, nil
	}

	team, err := GetTeamByID(db, strconv.Itoa(int(teamID)))
	if err != nil {
		return TeamAccess{}, err
	}

	if team.IsManualUpgrade {
		return TeamAccess{IsPro: true}, nil
	}

	sub, err := GetSubscriptionByTeamID(db, teamID)
	if err != nil {
		return TeamAccess{}, err
	}

	if sub != nil && sub.IsActive() {
		if sub.Status == StatusTrialing {
			ends := sub.CurrentPeriodEnd
			return TeamAccess{IsPro: true, IsTrial: true, TrialEndsAt: &ends}, nil
		}
		return TeamAccess{IsPro: true}, nil
	}

	// Teams created on/after the cutoff get no free trial. Access requires an
	// active/trialing Stripe subscription (handled above). This makes
	// checkUserHasAccess return false so the existing 402 paths enforce the
	// paywall without any new middleware.
	if IsTeamPostCutoff(team) {
		return TeamAccess{}, nil
	}

	const trialDays = 14
	ends := team.CreatedAt.AddDate(0, 0, trialDays)
	return TeamAccess{IsTrial: true, TrialEndsAt: &ends}, nil
}

// GetUserWithSubscription returns a user with subscription information.
// When stripeEnabled is false (self-hosted deployments without Stripe), every
// user is treated as Pro and the trial is bypassed.
func GetUserWithSubscription(db *gorm.DB, user *User, stripeEnabled bool) (*UserWithSubscription, error) {
	if !stripeEnabled {
		return &UserWithSubscription{User: *user, TeamAccess: TeamAccess{IsPro: true}}, nil
	}

	access, err := GetTeamAccess(db, *user.TeamID, stripeEnabled)
	if err != nil {
		return nil, err
	}

	return &UserWithSubscription{User: *user, TeamAccess: access}, nil
}
