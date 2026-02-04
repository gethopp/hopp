package handlers

import (
	"encoding/json"
	"fmt"
	"hopp-backend/internal/common"
	"hopp-backend/internal/config"
	"hopp-backend/internal/email"
	"hopp-backend/internal/models"
	"hopp-backend/internal/notifications"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/labstack/echo/v4"
	"github.com/stripe/stripe-go/v82"
	portalsession "github.com/stripe/stripe-go/v82/billingportal/session"
	checkoutsession "github.com/stripe/stripe-go/v82/checkout/session"
	"github.com/stripe/stripe-go/v82/customer"
	"github.com/stripe/stripe-go/v82/webhook"
	"gorm.io/gorm"
)

// BillingHandler handles Stripe billing and subscription operations
type BillingHandler struct {
	DB          *gorm.DB
	Config      *config.Config
	JwtIssuer   common.JWTIssuer
	EmailClient email.EmailClient
}

// SubscriptionResponse represents the subscription status response
type SubscriptionResponse struct {
	Status            models.SubscriptionStatus `json:"status"`
	ManualUpgrade     bool                      `json:"manual_upgrade"`
	CurrentPeriodEnd  *time.Time                `json:"current_period_end,omitempty"`
	CancelAtPeriodEnd *bool                     `json:"cancel_at_period_end,omitempty"`
	IsAdmin           bool                      `json:"is_admin"`
}

// BillingSettingsRequest represents the request body for updating billing settings
type BillingSettingsRequest struct {
	BillingEmail string `json:"billing_email" validate:"omitempty,email"`
}

// BillingSettingsResponse represents the response for billing settings
type BillingSettingsResponse struct {
	BillingEmail string `json:"billing_email"`
}

// TODO: Refactor billing and handlers to avoid complicated codebase
// Maybe share a common interface to implement

// NewBillingHandler creates a new billing handler with Stripe integration
func NewBillingHandler(db *gorm.DB, config *config.Config, jwtIssuer common.JWTIssuer, emailClient email.EmailClient) *BillingHandler {
	// Set Stripe API key
	stripe.Key = config.Stripe.SecretKey

	return &BillingHandler{
		DB:          db,
		Config:      config,
		JwtIssuer:   jwtIssuer,
		EmailClient: emailClient,
	}
}

// CreateCheckoutSession creates a Stripe checkout session for subscription
func (bh *BillingHandler) CreateCheckoutSession(c echo.Context) error {
	user, found := bh.getAuthenticatedUserFromJWT(c)
	if !found {
		return echo.NewHTTPError(http.StatusUnauthorized, "Failed to authenticate user")
	}

	// Parse request body
	var req struct {
		PriceID  string `json:"price_id,omitempty"`
		Tier     string `json:"tier" validate:"required"`
		Referral string `json:"referral,omitempty"` // Rewardful referral ID for affiliate tracking
	}

	if err := c.Bind(&req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid request body")
	}

	if err := c.Validate(&req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	// Determine the correct price ID
	var priceID string
	if req.PriceID != "" {
		priceID = req.PriceID
	} else {
		// Use the environment variable for paid tier
		if req.Tier == "paid" {
			priceID = bh.Config.Stripe.PaidPriceID
			if priceID == "" {
				return echo.NewHTTPError(http.StatusInternalServerError, "STRIPE_PAID_PRICE_ID environment variable is not configured")
			}
		} else {
			return echo.NewHTTPError(http.StatusBadRequest, "Invalid tier or missing price_id")
		}
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User must be part of a team to subscribe")
	}

	if !user.IsAdmin {
		return echo.NewHTTPError(http.StatusForbidden, "Only team admins can manage subscriptions")
	}

	// Get or create Stripe customer
	team, err := models.GetTeamByID(bh.DB, strconv.Itoa(int(*user.TeamID)))
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team")
	}

	// Check if team already has a subscription
	existingSub, err := models.GetSubscriptionByTeamID(bh.DB, *user.TeamID)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to check existing subscription")
	}

	if existingSub != nil && existingSub.IsActive() {
		return echo.NewHTTPError(http.StatusBadRequest, "Team already has an active subscription")
	}

	var stripeCustomerID string
	if existingSub != nil {
		stripeCustomerID = existingSub.StripeCustomerID
	} else {
		// Create Stripe customer
		customerParams := &stripe.CustomerParams{
			Email: stripe.String(user.Email),
			Name:  stripe.String(team.Name),
			Metadata: map[string]string{
				"team_id": strconv.Itoa(int(*user.TeamID)),
				"user_id": user.ID,
			},
		}

		stripeCustomer, err := customer.New(customerParams)
		if err != nil {
			return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create Stripe customer")
		}
		stripeCustomerID = stripeCustomer.ID
	}

	// Get how many team members are in the team
	teamMembers, err := models.GetTeamMembersByTeamID(bh.DB, *user.TeamID)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team members")
	}

	// Create checkout session
	params := &stripe.CheckoutSessionParams{
		Mode:     stripe.String(stripe.CheckoutSessionModeSubscription),
		Customer: stripe.String(stripeCustomerID),
		LineItems: []*stripe.CheckoutSessionLineItemParams{
			{
				Price:    stripe.String(priceID),
				Quantity: stripe.Int64(int64(len(teamMembers))),
			},
		},
		SuccessURL: stripe.String(bh.Config.Stripe.SuccessURL + "?session_id={CHECKOUT_SESSION_ID}"),
		CancelURL:  stripe.String(bh.Config.Stripe.CancelURL),
		Metadata: map[string]string{
			"team_id":    strconv.Itoa(int(*user.TeamID)),
			"tier":       req.Tier,
			"admin_id":   user.ID,
			"user_count": strconv.Itoa(len(teamMembers)),
		},
		AllowPromotionCodes:      stripe.Bool(true),
		BillingAddressCollection: stripe.String("auto"),
		CustomerUpdate: &stripe.CheckoutSessionCustomerUpdateParams{
			Name:    stripe.String("auto"),
			Address: stripe.String("auto"),
		},
		TaxIDCollection: &stripe.CheckoutSessionTaxIDCollectionParams{
			Enabled: stripe.Bool(true),
		},
	}

	// Pass Rewardful referral ID for affiliate tracking (only if present)
	// Stripe raises an error if client_reference_id is blank
	if req.Referral != "" {
		params.ClientReferenceID = stripe.String(req.Referral)
	}

	session, err := checkoutsession.New(params)
	if err != nil {
		c.Logger().Errorf("Stripe checkout session creation failed: %v", err)

		// Check if it's a price ID issue
		if strings.Contains(err.Error(), "No such price") {
			return echo.NewHTTPError(http.StatusInternalServerError,
				fmt.Sprintf("Invalid Stripe price ID configured: %s. Please check your STRIPE_PAID_PRICE_ID environment variable.", priceID))
		}

		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create checkout session")
	}

	return c.JSON(http.StatusOK, map[string]string{
		"checkout_url": session.URL,
		"session_id":   session.ID,
	})
}

// CreatePortalSession creates a Stripe billing portal session
func (bh *BillingHandler) CreatePortalSession(c echo.Context) error {
	user, found := bh.getAuthenticatedUserFromJWT(c)
	if !found {
		return echo.NewHTTPError(http.StatusUnauthorized, "Failed to authenticate user")
	}

	// Validate user has a team and is admin
	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User must be part of a team")
	}

	if !user.IsAdmin {
		return echo.NewHTTPError(http.StatusForbidden, "Only team admins can access billing portal")
	}

	// Get subscription
	subscription, err := models.GetSubscriptionByTeamID(bh.DB, *user.TeamID)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get subscription")
	}

	if subscription == nil {
		return echo.NewHTTPError(http.StatusNotFound, "No subscription found")
	}

	// Create portal session
	params := &stripe.BillingPortalSessionParams{
		Customer:  stripe.String(subscription.StripeCustomerID),
		ReturnURL: stripe.String(fmt.Sprintf("https://%s/subscription", bh.Config.Server.DeployDomain)),
	}

	portalSession, err := portalsession.New(params)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to create portal session")
	}

	return c.JSON(http.StatusOK, map[string]string{
		"portal_url": portalSession.URL,
	})
}

// GetSubscriptionStatus returns the current subscription status for the user's team
func (bh *BillingHandler) GetSubscriptionStatus(c echo.Context) error {
	user, found := bh.getAuthenticatedUserFromJWT(c)
	if !found {
		return echo.NewHTTPError(http.StatusUnauthorized, "Failed to authenticate user")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusInternalServerError)
	}

	// Get team with subscription
	team, err := models.GetTeamByID(bh.DB, strconv.Itoa(int(*user.TeamID)))
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team")
	}

	subscription, err := models.GetSubscriptionByTeamID(bh.DB, *user.TeamID)
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get subscription")
	}

	// There are cases:
	// User is manually upgraded
	// User's team is subscribed
	// No subscription, they are on free tier
	var subscriptionResponse SubscriptionResponse

	if team.IsManualUpgrade {
		subscriptionResponse = SubscriptionResponse{
			Status:            models.StatusActive,
			ManualUpgrade:     team.IsManualUpgrade,
			CurrentPeriodEnd:  nil,
			CancelAtPeriodEnd: nil,
			IsAdmin:           user.IsAdmin,
		}
	} else if subscription != nil {
		subscriptionResponse = SubscriptionResponse{
			Status:            subscription.Status,
			ManualUpgrade:     team.IsManualUpgrade,
			CurrentPeriodEnd:  &subscription.CurrentPeriodEnd,
			CancelAtPeriodEnd: &subscription.CancelAtPeriodEnd,
			IsAdmin:           user.IsAdmin,
		}
	} else {
		subscriptionResponse = SubscriptionResponse{
			Status:            models.StatusTrialing,
			ManualUpgrade:     team.IsManualUpgrade,
			CurrentPeriodEnd:  nil,
			CancelAtPeriodEnd: nil,
			IsAdmin:           user.IsAdmin,
		}
	}

	return c.JSON(http.StatusOK, map[string]interface{}{
		"subscription": subscriptionResponse,
	})
}

// HandleWebhook handles Stripe webhook events
func (bh *BillingHandler) HandleWebhook(c echo.Context) error {
	const MaxBodyBytes = int64(65536)
	body := http.MaxBytesReader(c.Response(), c.Request().Body, MaxBodyBytes)
	payload, err := io.ReadAll(body)
	if err != nil {
		return echo.NewHTTPError(http.StatusServiceUnavailable, "Error reading request body")
	}

	// Verify webhook signature
	event, err := webhook.ConstructEvent(payload, c.Request().Header.Get("Stripe-Signature"), bh.Config.Stripe.WebhookSecret)
	if err != nil {
		c.Logger().Errorf("Webhook signature verification failed: %v", err)
		return echo.NewHTTPError(http.StatusBadRequest, "Webhook signature verification failed")
	}

	// Handle the event
	// All events are here:
	// https://docs.stripe.com/api/events/types
	switch event.Type {
	case "customer.subscription.created":
		err = bh.handleSubscriptionCreated(c, event)
	case "customer.subscription.updated":
		err = bh.handleSubscriptionUpdated(c, event)
	case "checkout.session.completed":
		err = bh.handleCheckoutSessionCompleted(c, event)
	case "invoice.payment_succeeded":
		err = bh.handleInvoicePaymentSucceeded(c, event)
	default:
		c.Logger().Infof("Unhandled event type: %s", event.Type)
	}

	if err != nil {
		c.Logger().Errorf("Error handling webhook event %s: %v", event.Type, err)
		return echo.NewHTTPError(http.StatusInternalServerError, "Error processing webhook")
	}

	return c.NoContent(http.StatusOK)
}

// Noop for now
func (bh *BillingHandler) handleSubscriptionCreated(c echo.Context, event stripe.Event) error {
	var subscription stripe.Subscription
	if err := json.Unmarshal(event.Data.Raw, &subscription); err != nil {
		return err
	}

	return nil
}

// Changes in subscription like cancelling
func (bh *BillingHandler) handleSubscriptionUpdated(c echo.Context, event stripe.Event) error {
	var subscription stripe.Subscription
	if err := json.Unmarshal(event.Data.Raw, &subscription); err != nil {
		return err
	}

	dbSub, err := models.GetSubscriptionByStripeID(bh.DB, subscription.ID)
	if err != nil {
		c.Logger().Errorf("Failed to get subscription by stripe ID: %v", err)
		return err
	}

	if dbSub == nil {
		c.Logger().Errorf("Subscription not found in database: %s", subscription.ID)
		return nil
	}

	if subscription.CancelAtPeriodEnd {
		dbSub.Status = models.StatusCanceled
		if err := bh.DB.Save(dbSub).Error; err != nil {
			c.Logger().Errorf("Failed to save subscription: %v", err)
			return err
		}

		_ = notifications.SendTelegramNotification(fmt.Sprintf("Team ID: %s - subscription cancelled", strconv.Itoa(int(dbSub.TeamID))), bh.Config)

		// Send email to admin user for acknowledgement of cancellation
		adminUser, err := models.GetAdminUserForTeam(bh.DB, dbSub.TeamID)
		if err != nil {
			c.Logger().Errorf("Failed to get admin user for team: %v", err)
		} else if adminUser != nil && bh.EmailClient != nil {
			c.Logger().Infof("Sending subscription cancellation email to admin user: %s", adminUser.Email)
			bh.EmailClient.SendSubscriptionCancellationEmail(adminUser)
			_ = notifications.SendTelegramNotification(fmt.Sprintf("🥲🥲🥲 Team ID: %s - subscription cancelled", strconv.Itoa(int(dbSub.TeamID))), bh.Config)
		}
	}

	// Revoking cancelled subscription
	if !subscription.CancelAtPeriodEnd && event.GetPreviousValue("cancel_at_period_end") == "true" {
		dbSub.Status = models.StatusActive
		if err := bh.DB.Save(dbSub).Error; err != nil {
			c.Logger().Errorf("Failed to save subscription: %v", err)
			return err
		}

		c.Logger().Infof("Revoking cancelled subscription: %s", subscription.ID)
		_ = notifications.SendTelegramNotification(fmt.Sprintf("Team ID: %s - subscription cancellation revoked", strconv.Itoa(int(dbSub.TeamID))), bh.Config)
	}

	return nil
}

func (bh *BillingHandler) handleCheckoutSessionCompleted(c echo.Context, event stripe.Event) error {
	c.Logger().Infof("Handling checkout session completed event: %s", event.ID)
	var session stripe.CheckoutSession
	if err := json.Unmarshal(event.Data.Raw, &session); err != nil {
		return err
	}

	// If there's no subscription, this wasn't a subscription checkout
	if session.Subscription == nil {
		return nil
	}

	teamIDStr := session.Metadata["team_id"]
	if teamIDStr == "" {
		return fmt.Errorf("team_id not found in subscription metadata")
	}

	teamID, err := strconv.Atoi(teamIDStr)
	if err != nil {
		return err
	}

	// Determine tier from price ID
	tier := models.SubscriptionTier(session.Metadata["tier"])

	// Get or create subscription record
	dbSub, err := models.GetSubscriptionByStripeID(bh.DB, session.Subscription.ID)
	if err != nil && err != gorm.ErrRecordNotFound {
		return err
	}

	if dbSub == nil {
		c.Logger().Infof("Creating new subscription: %+v", dbSub)
		// Create new subscription
		dbSub = &models.Subscription{
			TeamID:               uint(teamID),
			StripeCustomerID:     session.Customer.ID,
			StripeSubscriptionID: session.Subscription.ID,
		}
	}

	// Update subscription fields
	dbSub.Status = models.StatusActive
	dbSub.Tier = tier
	// Note: Stripe subscription doesn't have direct CurrentPeriodStart/End fields
	// These would typically come from the invoice or billing cycle
	dbSub.CurrentPeriodStart = time.Unix(session.Created, 0)
	dbSub.CurrentPeriodEnd = time.Unix(session.Created, 0).AddDate(0, 1, 0) // Assume monthly
	dbSub.CancelAtPeriodEnd = session.Subscription.CancelAtPeriodEnd

	if session.Subscription.CanceledAt != 0 {
		canceledAt := time.Unix(session.Subscription.CanceledAt, 0)
		dbSub.CanceledAt = &canceledAt
	}

	// Update team tier
	team, err := models.GetTeamByID(bh.DB, strconv.Itoa(teamID))
	if err != nil {
		return err
	}

	// Save in transaction
	err = bh.DB.Transaction(func(tx *gorm.DB) error {
		if err := tx.Save(dbSub).Error; err != nil {
			return err
		}
		return tx.Save(team).Error
	})
	if err != nil {
		c.Logger().Errorf("Failed to save subscription/team in transaction: %v", err)
		return err
	}

	c.Logger().Infof("Subscription saved for team: %s", team.Name)

	adminUser, err := models.GetAdminUserForTeam(bh.DB, dbSub.TeamID)
	if err != nil {
		c.Logger().Errorf("Failed to get admin user for team: %v", err)
	} else if adminUser != nil && bh.EmailClient != nil {
		c.Logger().Infof("Sending subscription confirmation email to admin user: %s", adminUser.Email)
		bh.EmailClient.SendSubscriptionConfirmationEmail(adminUser)
	}

	_ = notifications.SendTelegramNotification(fmt.Sprintf("💸💸💸 Team ID: %s - subscription activated", strconv.Itoa(int(dbSub.TeamID))), bh.Config)

	return nil
}

// handleInvoicePaymentSucceeded handles the invoice.payment_succeeded webhook event
// It sends an invoice email to the team's billing email if configured
func (bh *BillingHandler) handleInvoicePaymentSucceeded(c echo.Context, event stripe.Event) error {
	var invoice stripe.Invoice
	if err := json.Unmarshal(event.Data.Raw, &invoice); err != nil {
		c.Logger().Errorf("Failed to unmarshal invoice: %v", err)
		return err
	}

	// Skip if invoice has no customer
	if invoice.Customer == nil || invoice.Customer.ID == "" {
		c.Logger().Info("Invoice has no customer, skipping invoice email")
		return nil
	}

	// Look up subscription by Stripe customer ID
	var subscription models.Subscription
	if err := bh.DB.Preload("Team").Where("stripe_customer_id = ?", invoice.Customer.ID).First(&subscription).Error; err != nil {
		c.Logger().Infof("Could not find subscription for customer %s: %v", invoice.Customer.ID, err)
		// Return nil to acknowledge the webhook as subscription might not exist yet
		return nil
	}

	team := subscription.Team

	// Only send if team has billing
	// email set else skip
	if team.BillingEmail == nil || *team.BillingEmail == "" {
		c.Logger().Infof("Team %d has no billing email set, skipping invoice email", team.ID)
		return nil
	}

	// Format period from timestamps (period_start, period_end)
	periodStart := time.Unix(invoice.PeriodStart, 0)
	periodEnd := time.Unix(invoice.PeriodEnd, 0)
	period := fmt.Sprintf("%s - %s", periodStart.Format("Jan 2"), periodEnd.Format("Jan 2, 2006"))

	// Send invoice email
	if bh.EmailClient != nil {
		bh.EmailClient.SendInvoiceEmail(*team.BillingEmail, email.InvoiceEmailData{
			InvoiceNumber:    invoice.Number,
			Period:           period,
			HostedInvoiceURL: invoice.HostedInvoiceURL,
			InvoicePDFURL:    invoice.InvoicePDF,
		})
		c.Logger().Infof("Sent invoice email for %s to %s", invoice.Number, *team.BillingEmail)
	}

	return nil
}

// GetBillingSettings returns the current billing settings for the user's team
func (bh *BillingHandler) GetBillingSettings(c echo.Context) error {
	user, found := bh.getAuthenticatedUserFromJWT(c)
	if !found {
		return echo.NewHTTPError(http.StatusUnauthorized, "Failed to authenticate user")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User must be part of a team")
	}

	if !user.IsAdmin {
		return echo.NewHTTPError(http.StatusForbidden, "Only team admins can view billing settings")
	}

	// Get team
	team, err := models.GetTeamByID(bh.DB, strconv.Itoa(int(*user.TeamID)))
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team")
	}

	billingEmail := ""
	if team.BillingEmail != nil {
		billingEmail = *team.BillingEmail
	}

	return c.JSON(http.StatusOK, BillingSettingsResponse{
		BillingEmail: billingEmail,
	})
}

// UpdateBillingSettings updates the billing settings for the user's team
func (bh *BillingHandler) UpdateBillingSettings(c echo.Context) error {
	user, found := bh.getAuthenticatedUserFromJWT(c)
	if !found {
		return echo.NewHTTPError(http.StatusUnauthorized, "Failed to authenticate user")
	}

	if user.TeamID == nil {
		return echo.NewHTTPError(http.StatusBadRequest, "User must be part of a team")
	}

	if !user.IsAdmin {
		return echo.NewHTTPError(http.StatusForbidden, "Only team admins can update billing settings")
	}

	var req BillingSettingsRequest
	if err := c.Bind(&req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, "Invalid request body")
	}

	if err := c.Validate(&req); err != nil {
		return echo.NewHTTPError(http.StatusBadRequest, err.Error())
	}

	// Get team
	team, err := models.GetTeamByID(bh.DB, strconv.Itoa(int(*user.TeamID)))
	if err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to get team")
	}

	// Update billing email (allow empty string to clear it)
	if req.BillingEmail == "" {
		team.BillingEmail = nil
	} else {
		team.BillingEmail = &req.BillingEmail
	}

	if err := bh.DB.Save(team).Error; err != nil {
		return echo.NewHTTPError(http.StatusInternalServerError, "Failed to update billing settings")
	}

	billingEmail := ""
	if team.BillingEmail != nil {
		billingEmail = *team.BillingEmail
	}

	return c.JSON(http.StatusOK, BillingSettingsResponse{
		BillingEmail: billingEmail,
	})
}
