package handlers

import (
	"testing"

	"hopp-backend/internal/models"

	"github.com/stretchr/testify/assert"
	"github.com/stripe/stripe-go/v82"
)

// TestMapStripeSubscriptionStatus covers the status mapping that drives access
// (IsActive), the trial-vs-confirmation email branch, and the webhook status
// sync. Unknown statuses default to active so access is never accidentally lost.
func TestMapStripeSubscriptionStatus(t *testing.T) {
	cases := []struct {
		in   stripe.SubscriptionStatus
		want models.SubscriptionStatus
	}{
		{stripe.SubscriptionStatusTrialing, models.StatusTrialing},
		{stripe.SubscriptionStatusActive, models.StatusActive},
		{stripe.SubscriptionStatusPastDue, models.StatusPastDue},
		{stripe.SubscriptionStatusCanceled, models.StatusCanceled},
		{stripe.SubscriptionStatusUnpaid, models.StatusCanceled},
		{stripe.SubscriptionStatusIncomplete, models.StatusIncomplete},
		{stripe.SubscriptionStatusIncompleteExpired, models.StatusIncomplete},
		{stripe.SubscriptionStatusPaused, models.StatusActive},
	}

	for _, tc := range cases {
		t.Run(string(tc.in), func(t *testing.T) {
			assert.Equal(t, tc.want, mapStripeSubscriptionStatus(tc.in))
		})
	}
}
