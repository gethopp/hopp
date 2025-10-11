# Some dev notes

Test webhooks locally with event forwarding instead of exposing a URL

```bash
stripe login


# This created a webhook secret, then add this to the .env.local file
stripe listen --forward-to https://localhost:1926/api/billing/webhook

# Manual trigger an event if needed
stripe trigger payment_intent.succeeded

# Trigger again an event, grab the event id from Stripe's Dashboard
stripe events resend evt_XXXXXXXXXXX
```

# Testing subscription flow

For testing a subscription flow, visit the web-app and add details that can
[be found here](https://docs.stripe.com/testing?testing-method=card-numbers#cards).

## Bypass trial

There are internal flags we use in team model, called `is_manual_upgrade` which
can be set to true to bypass the trial period.
