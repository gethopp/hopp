package models

import (
	"time"

	"gorm.io/gorm"
)

// SubscriptionTier represents different subscription tiers
type SubscriptionTier string

const (
	TierFree SubscriptionTier = "free"
	TierPaid SubscriptionTier = "paid"
)

// SubscriptionStatus represents the status of a subscription
type SubscriptionStatus string

const (
	StatusActive     SubscriptionStatus = "active"
	StatusCanceled   SubscriptionStatus = "canceled"
	StatusPastDue    SubscriptionStatus = "past_due"
	StatusTrialing   SubscriptionStatus = "trialing"
	StatusIncomplete SubscriptionStatus = "incomplete"
)

// Subscription represents a team's subscription to a plan
type Subscription struct {
	gorm.Model
	TeamID               uint               `gorm:"not null;uniqueIndex" json:"team_id"`
	Team                 Team               `json:"team,omitempty"`
	StripeCustomerID     string             `gorm:"not null" json:"stripe_customer_id"`
	StripeSubscriptionID string             `gorm:"not null;unique" json:"stripe_subscription_id"`
	Status               SubscriptionStatus `gorm:"not null" json:"status"`
	Tier                 SubscriptionTier   `gorm:"not null;default:'free'" json:"tier"`
	CurrentPeriodStart   time.Time          `json:"current_period_start"`
	CurrentPeriodEnd     time.Time          `json:"current_period_end"`
	CancelAtPeriodEnd    bool               `gorm:"default:false" json:"cancel_at_period_end"`
	CanceledAt           *time.Time         `json:"canceled_at,omitempty"`
	// Usage tracking for billing
	LastBilledUserCount int       `json:"last_billed_user_count"`
	CurrentUserCount    int       `json:"current_user_count"`
	LastUsageUpdate     time.Time `json:"last_usage_update"`
}

// GetSubscriptionByTeamID retrieves a subscription by team ID
func GetSubscriptionByTeamID(db *gorm.DB, teamID uint) (*Subscription, error) {
	var subscription Subscription
	result := db.Where("team_id = ?", teamID).First(&subscription)

	if result.Error != nil {
		if result.Error == gorm.ErrRecordNotFound {
			return nil, nil // No subscription found, which is valid for free tier
		}
		return nil, result.Error
	}

	return &subscription, nil
}

// IsActive returns true if the subscription is active
func (s *Subscription) IsActive() bool {
	return s.Status == StatusActive || s.Status == StatusTrialing
}

func GetSubscriptionByStripeID(db *gorm.DB, stripeSubscriptionID string) (*Subscription, error) {
	var subscription Subscription
	result := db.Where("stripe_subscription_id = ?", stripeSubscriptionID).First(&subscription)

	if result.Error != nil {
		return nil, result.Error
	}

	return &subscription, nil
}
