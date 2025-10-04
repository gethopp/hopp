# Some dev notes

Test webhooks locally with event forwarding instead of exposing a URL

```bash
stripe login


# This created a webhook secret, then add this to the .env.local file
stripe listen --forward-to https://localhost:1926/api/billing/webhook

# Manual trigger an event if needed
stripe trigger payment_intent.succeeded
```

# Stripe Integration Setup

This document provides instructions for setting up the Stripe integration for subscription management in the Hopp application.

## Prerequisites

1. A Stripe account (https://stripe.com)
2. Products and prices configured in your Stripe Dashboard
3. Webhook endpoint configured in Stripe

## Environment Variables

Add the following environment variables to your backend configuration:

```bash
# Stripe Configuration
STRIPE_SECRET_KEY=sk_test_your_stripe_secret_key_here
STRIPE_PUBLISHABLE_KEY=pk_test_your_stripe_publishable_key_here
STRIPE_WEBHOOK_SECRET=whsec_your_webhook_secret_here

# Stripe Price ID for paid tier (usage-based)
STRIPE_PAID_PRICE_ID=price_paid_plan_id

# Domain for Stripe redirects (optional)
STRIPE_SUCCESS_URL=http://localhost:3000/subscription/success
STRIPE_CANCEL_URL=http://localhost:3000/subscription/cancel
```

## Stripe Dashboard Setup

### 1. Create Products and Prices

Create the following product in your Stripe Dashboard:

#### Paid Plan (Usage-Based)

- **Name**: Paid Plan
- **Billing Model**: Per-seat or usage-based
- **Price**: $5 per user per month
- **Lookup Key**: `paid_plan` (recommended)
- Copy the Price ID to `STRIPE_PAID_PRICE_ID`

**Note**: This is a simplified pricing model with only two tiers:

- **Free**: Up to 5 team members, 3 rooms
- **Paid**: $5 per team member per month, unlimited rooms and members

### 2. Configure Webhook Endpoint

1. Go to **Developers > Webhooks** in your Stripe Dashboard
2. Click **Add endpoint**
3. Set the endpoint URL to: `https://your-domain.com/api/billing/webhook`
4. Select the following events to listen for:
   - `customer.subscription.created`
   - `customer.subscription.updated`
   - `customer.subscription.deleted`
   - `customer.subscription.trial_will_end`
   - `checkout.session.completed`
5. Copy the webhook signing secret to `STRIPE_WEBHOOK_SECRET`

### 3. Enable Customer Portal (Optional)

1. Go to **Settings > Billing > Customer portal**
2. Activate the customer portal
3. Configure the allowed features (cancel subscriptions, update payment methods, etc.)

## API Endpoints

The following API endpoints are available for Stripe integration:

### Public Endpoints

- `GET /api/billing/pricing` - Get pricing information
- `POST /api/billing/webhook` - Stripe webhook handler

### Protected Endpoints (Require Authentication)

- `GET /api/auth/billing/subscription` - Get current subscription status
- `POST /api/auth/billing/create-checkout-session` - Create Stripe checkout session
- `POST /api/auth/billing/create-portal-session` - Create billing portal session
- `POST /api/auth/billing/update-usage` - Update current user count for billing
- `POST /api/auth/billing/manual-upgrade` - Manually upgrade a team (admin only)

## Frontend Integration

The subscription management is available in the web app:

1. **Admin-only Access**: Only team administrators can access the subscription page
2. **Subscription Tab**: A "Subscription" tab appears in the sidebar for admin users
3. **Pricing Display**: Shows all available tiers with features and pricing
4. **Current Plan**: Displays the team's current subscription status
5. **Upgrade/Manage**: Allows admins to upgrade plans or manage billing

## Database Models

### Subscription Model

The `Subscription` model tracks subscription details:

- Team association
- Stripe customer and subscription IDs
- Subscription status and tier
- Billing period information
- Trial information

### Team Model Updates

The `Team` model now includes:

- `Tier` field indicating the subscription tier
- Relationship to `Subscription` model

## Subscription Tiers and Features

### Free Tier

- **Price**: $0
- **Max Rooms**: 3
- **Max Team Members**: 5
- **Features**: Basic video calls, Screen sharing, Chat

### Paid Tier

- **Price**: $5 per team member per month
- **Max Rooms**: Unlimited
- **Max Team Members**: Unlimited
- **Features**: Everything in Free + Unlimited rooms, Unlimited team members, Recording, Custom backgrounds, Priority support, Advanced analytics

## Manual Upgrades

Teams can be manually upgraded without billing using the `IsManualUpgrade` flag:

- Manual upgrades give teams access to paid features without Stripe billing
- Useful for special cases, partnerships, or internal teams
- Can be managed via the `/api/auth/billing/manual-upgrade` endpoint

## Testing

### Test Mode

Use Stripe test keys for development:

- Test Secret Key: `sk_test_...`
- Test Publishable Key: `pk_test_...`

### Test Cards

Use Stripe's test card numbers:

- **Success**: `4242 4242 4242 4242`
- **Decline**: `4000 0000 0000 0002`
- **Authentication Required**: `4000 0025 0000 3155`

## Security Considerations

1. **Environment Variables**: Never commit Stripe keys to version control
2. **Webhook Verification**: Always verify webhook signatures
3. **Admin Access**: Subscription management is restricted to team admins
4. **HTTPS**: Use HTTPS in production for webhook endpoints

## Troubleshooting

### Common Issues

1. **Webhook Signature Verification Failed**

   - Ensure `STRIPE_WEBHOOK_SECRET` is correctly set
   - Check that the webhook endpoint URL is accessible

2. **Price ID Not Found**

   - Verify the price IDs in your environment variables
   - Ensure the prices exist in your Stripe account

3. **Subscription Page Not Visible**
   - Check that the user is marked as an admin (`is_admin: true`)
   - Verify the user is part of a team

### Logs

Check the backend logs for Stripe-related errors. The application logs webhook events and API interactions for debugging.

## Support

For additional support with Stripe integration:

1. Check the [Stripe Documentation](https://stripe.com/docs)
2. Review the [Stripe Go SDK](https://github.com/stripe/stripe-go)
3. Contact your development team for application-specific issues
