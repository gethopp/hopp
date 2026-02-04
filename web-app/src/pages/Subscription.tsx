import { useState, useEffect } from "react";
import { useHoppStore } from "@/store/store";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { toast } from "react-hot-toast";
import { HiXMark } from "react-icons/hi2";
import { HiExclamationCircle } from "react-icons/hi2";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useAPI } from "@/hooks/useQueryClients";
import { useQueryClient } from "@tanstack/react-query";
import type { components } from "@/openapi";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import clsx from "clsx";
import { FaCheck } from "react-icons/fa";
import { PrimaryCTA } from "@/components/ui/atomic/Buttons";
import { getRewardfulReferral } from "@/lib/rewardful";

type SubscriptionResponse = components["schemas"]["SubscriptionResponse"];

const tiers = [
  {
    name: "Cracked teams",
    id: "tier-cracked",
    href: "#",
    priceMonthly: "$8",
    description: "Perfect for engineering teams who want to ship faster and collaborate better.",
    features: [
      "Unlimited pair programming sessions",
      "Support the only low-latency OSS screen sharing app",
      "Social auth with Google, Slack support",
      "<1 day support guarantee",
    ],
    featured: true,
  },
  {
    name: "Enterprise",
    id: "tier-enterprise",
    href: "#",
    priceMonthly: "Contact us",
    description: "Advanced features and support for large organizations.",
    features: ["Everything in Cracked teams", "Single sign-on (SSO)", "Custom invoicing", "Volume pricing discount"],
    featured: false,
  },
];

export function Subscription() {
  const { authToken } = useHoppStore();
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [billingEmail, setBillingEmail] = useState("");
  const [billingEmailSaving, setBillingEmailSaving] = useState(false);

  const { useQuery, useMutation } = useAPI();
  const queryClient = useQueryClient();

  const { data: subscriptionData, isLoading: loading } = useQuery("get", "/api/auth/billing/subscription", undefined, {
    queryHash: `subscription-${authToken}`,
  });

  const subscriptionStatus = subscriptionData?.subscription;

  // Fetch billing settings
  const { data: billingSettingsData, isLoading: billingSettingsLoading } = useQuery(
    "get",
    "/api/auth/billing/settings",
    undefined,
    {
      queryHash: `billing-settings-${authToken}`,
      enabled: !!subscriptionStatus?.is_admin,
    },
  );

  // Update local state when billing settings are loaded
  useEffect(() => {
    if (billingSettingsData?.billing_email !== undefined) {
      setBillingEmail(billingSettingsData.billing_email);
    }
  }, [billingSettingsData?.billing_email]);

  const createCheckoutSessionMutation = useMutation("post", "/api/auth/billing/create-checkout-session");

  const createPortalSessionMutation = useMutation("post", "/api/auth/billing/create-portal-session");

  const updateBillingSettingsMutation = useMutation("put", "/api/auth/billing/settings");

  const handleUpgrade = async (tier: string) => {
    if (!subscriptionStatus?.is_admin) {
      toast.error("Only team admins can manage subscriptions");
      return;
    }

    setActionLoading(tier);
    try {
      const referral = getRewardfulReferral();

      const response = await createCheckoutSessionMutation.mutateAsync({
        body: {
          tier: tier as "paid",
          ...(referral && { referral }),
        },
      });

      if (response) {
        window.location.href = response.checkout_url;
      }
    } catch (error: unknown) {
      console.error("Error creating checkout session:", error);
      const errorMessage = error instanceof Error ? error.message : "Failed to create checkout session";
      toast.error(errorMessage);
    } finally {
      setActionLoading(null);
    }
  };

  const handleManageBilling = async () => {
    if (!subscriptionStatus?.is_admin) {
      toast.error("Only team admins can manage billing");
      return;
    }

    setActionLoading("portal");
    try {
      const response = await createPortalSessionMutation.mutateAsync({});

      if (response) {
        window.location.href = response.portal_url;
      }
    } catch (error: unknown) {
      console.error("Error creating portal session:", error);
      const errorMessage = error instanceof Error ? error.message : "Failed to create portal session";
      toast.error(errorMessage);
    } finally {
      setActionLoading(null);
    }
  };

  const handleEnterpriseContact = () => {
    window.location.href = "mailto:costa@gethopp.app?subject=Enterprise%20Plan%20Inquiry";
  };

  const handleSaveBillingEmail = async () => {
    setBillingEmailSaving(true);
    try {
      await updateBillingSettingsMutation.mutateAsync({
        body: {
          billing_email: billingEmail,
        },
      });
      // Invalidate the billing settings query to refetch
      queryClient.invalidateQueries({ queryKey: ["get", "/api/auth/billing/settings"] });
      toast.success("Billing email saved");
    } catch (error: unknown) {
      console.error("Error saving billing email:", error);
      const errorMessage = error instanceof Error ? error.message : "Failed to save billing email";
      toast.error(errorMessage);
    } finally {
      setBillingEmailSaving(false);
    }
  };

  const handleDeleteBillingEmail = async () => {
    setBillingEmailSaving(true);
    try {
      await updateBillingSettingsMutation.mutateAsync({
        body: {
          billing_email: "",
        },
      });
      setBillingEmail("");
      // Invalidate the billing settings query to refetch
      queryClient.invalidateQueries({ queryKey: ["get", "/api/auth/billing/settings"] });
      toast.success("Billing email removed");
    } catch (error: unknown) {
      console.error("Error removing billing email:", error);
      const errorMessage = error instanceof Error ? error.message : "Failed to remove billing email";
      toast.error(errorMessage);
    } finally {
      setBillingEmailSaving(false);
    }
  };

  // Determine the current state of the billing email form
  const savedBillingEmail = billingSettingsData?.billing_email || "";
  const hasSavedEmail = savedBillingEmail !== "";
  const isEmailModified = billingEmail !== savedBillingEmail;

  const getTierBadgeVariant = (tier: string) => {
    switch (tier) {
      case "free":
        return "secondary";
      case "paid":
        return "outline";
      default:
        return "secondary";
    }
  };

  // Helper function to determine tier based on subscription status
  const getTier = (subscription: SubscriptionResponse): string => {
    if (subscription.manual_upgrade) return "paid";
    if (subscription.status === "active") return "paid";

    // If subscription is canceled but still within the current period, treat as paid
    if (subscription.status === "canceled" && subscription.current_period_end) {
      const periodEnd = new Date(subscription.current_period_end);
      const now = new Date();
      if (periodEnd > now) return "paid";
    }

    return "free";
  };

  if (loading) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold">Subscription</h1>
          <p className="text-muted-foreground">Manage your team's subscription and billing</p>
        </div>
        <div className="flex justify-center items-center h-64">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary"></div>
        </div>
      </div>
    );
  }

  // Check if user is admin
  if (!subscriptionStatus?.is_admin) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold">Subscription</h1>
          <p className="text-muted-foreground">Only team administrators can access this page</p>
        </div>
        <Card className="max-w-md">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <HiXMark className="h-5 w-5 text-red-500" />
              Access Denied
            </CardTitle>
            <CardDescription>
              You need to be a team administrator to manage subscriptions. Please contact your team admin for access.
            </CardDescription>
          </CardHeader>
        </Card>
      </div>
    );
  }

  // Helper function to check if user has an active subscription (including canceled but still within period)
  const hasActiveSubscription = (subscription: SubscriptionResponse): boolean => {
    if (subscription.manual_upgrade) return true;
    if (subscription.status === "active") return true;

    // If subscription is canceled but still within the current period, treat as active
    if (subscription.status === "canceled" && subscription.current_period_end) {
      const periodEnd = new Date(subscription.current_period_end);
      const now = new Date();
      return periodEnd > now;
    }

    return false;
  };

  // If user has an active subscription (including canceled but still within period), show subscription details
  if (subscriptionStatus && hasActiveSubscription(subscriptionStatus)) {
    return (
      <div className="space-y-6">
        <div>
          <h1 className="text-3xl font-bold">Subscription</h1>
          <p className="text-muted-foreground">Manage your team's subscription and billing</p>
        </div>

        {/* Current Subscription Status */}
        <Card className="max-w-md">
          <CardHeader>
            <CardTitle className="flex items-center justify-between">
              <span>Current Plan</span>
              <Badge variant={getTierBadgeVariant(getTier(subscriptionStatus))}>
                {getTier(subscriptionStatus).charAt(0).toUpperCase() + getTier(subscriptionStatus).slice(1)}
              </Badge>
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-4">
              <div className="space-y-2">
                <p className="text-sm w-full flex flex-row justify-between">
                  <span className="font-medium">Status:</span>{" "}
                  <Badge className="ml-auto" variant={getTierBadgeVariant(getTier(subscriptionStatus))}>
                    {(
                      subscriptionStatus.status === "canceled" &&
                      subscriptionStatus.current_period_end &&
                      new Date(subscriptionStatus.current_period_end) > new Date()
                    ) ?
                      "Active (Canceled)"
                    : subscriptionStatus.status}
                  </Badge>
                </p>
                {subscriptionStatus.manual_upgrade && (
                  <p className="text-sm text-blue-600">
                    <span className="font-medium">Manual Upgrade:</span> This team has been manually upgraded
                  </p>
                )}
                {subscriptionStatus.current_period_end && (
                  <p className="text-sm w-full flex flex-row justify-between">
                    <span className="font-medium">
                      {subscriptionStatus.status === "canceled" ? "Subscription ends:" : "Next billing date:"}
                    </span>{" "}
                    <span className="font-normal text-slate-500">
                      {new Date(subscriptionStatus.current_period_end).toLocaleDateString()}
                    </span>
                  </p>
                )}
              </div>
              <Button
                onClick={handleManageBilling}
                disabled={actionLoading === "portal"}
                className="flex items-center gap-2"
              >
                {actionLoading === "portal" ? "Loading..." : "Manage subscription"}
              </Button>
            </div>
          </CardContent>
        </Card>

        {subscriptionStatus.status === "canceled" &&
          subscriptionStatus.current_period_end &&
          new Date(subscriptionStatus.current_period_end) > new Date() && (
            <Alert variant="default" className="max-w-md">
              <HiExclamationCircle className="size-5 -mt-1.5" />
              <AlertTitle>Cancelled subscription</AlertTitle>
              <AlertDescription>
                You subscription has been canceled but remains active until the end of your current billing period.
              </AlertDescription>
            </Alert>
          )}

        {/* Invoice Settings */}
        <Card className="max-w-md">
          <CardHeader>
            <CardTitle>Invoice Settings</CardTitle>
            <CardDescription>
              Configure where invoices are sent. This is separate from your Stripe account email.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                handleSaveBillingEmail();
              }}
              className="space-y-4"
            >
              <div className="space-y-2">
                <label htmlFor="billing-email" className="text-sm font-medium">
                  Billing Email
                </label>
                <Input
                  id="billing-email"
                  type="email"
                  placeholder="finance@yourcompany.com"
                  value={billingEmail}
                  onChange={(e) => setBillingEmail(e.target.value)}
                  disabled={billingSettingsLoading}
                />
                <p className="text-xs text-muted-foreground">
                  Invoices will be sent to this email address. Leave empty to not receive invoice emails.
                </p>
              </div>
              {hasSavedEmail && !isEmailModified ?
                <Button
                  type="button"
                  variant="destructive"
                  onClick={handleDeleteBillingEmail}
                  disabled={billingEmailSaving}
                >
                  {billingEmailSaving ? "Deleting..." : "Delete"}
                </Button>
              : <Button type="submit" disabled={billingEmailSaving || (!isEmailModified && !billingEmail)}>
                  {billingEmailSaving ? "Saving" : "Save"}
                </Button>
              }
            </form>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Otherwise show the pricing page for non-subscribed users
  return (
    <div className="relative isolate bg-white px-6 py-12 sm:py-16 lg:px-8">
      <div className="mx-auto max-w-4xl text-center">
        <h2 className="text-center text-4xl font-bold mb-8">Upgrade your team's subscription</h2>
      </div>

      <div className="mx-auto mt-8 grid max-w-lg grid-cols-1 items-center gap-y-6 sm:mt-12 sm:gap-y-0 lg:max-w-4xl lg:grid-cols-2">
        {tiers.map((tier, tierIdx) => (
          <div
            key={tier.id}
            className={clsx(
              tier.featured ? "relative bg-white shadow-2xl scale-[1.05]" : "bg-white/60 sm:mx-8 lg:mx-0",
              tier.featured ? ""
              : tierIdx === 0 ? "rounded-t-3xl sm:rounded-b-none lg:rounded-tr-none lg:rounded-bl-3xl"
              : "sm:rounded-t-none lg:rounded-tr-3xl lg:rounded-bl-none",
              "rounded-3xl p-8 ring-1 ring-gray-900/10 sm:p-10",
            )}
          >
            <h5 id={tier.id} className="font-semibold text-indigo-600">
              {tier.name}
            </h5>
            <p className="mt-4 flex items-baseline gap-x-2">
              <span className="text-5xl font-semibold tracking-tight text-gray-900">{tier.priceMonthly}</span>
              {tier.priceMonthly !== "Contact us" && <span className="text-base text-gray-500">/month/user</span>}
            </p>
            <p className="mt-6 text-base/7 text-gray-600">{tier.description}</p>
            <ul role="list" className="mt-8 space-y-3 text-sm/6 text-gray-600 sm:mt-10">
              {tier.features.map((feature) => (
                <li key={feature} className="flex gap-x-3">
                  <FaCheck aria-hidden="true" className="h-6 w-5 flex-none text-indigo-600" />
                  {feature}
                </li>
              ))}
            </ul>
            <PrimaryCTA
              onClick={() => {
                if (tier.name === "Enterprise") {
                  handleEnterpriseContact();
                } else {
                  handleUpgrade("paid");
                }
              }}
              disabled={actionLoading === tier.id}
              aria-describedby={tier.id}
              className={clsx(tier.featured ? "" : "", "mt-8")}
              fill={tier.featured ? "filled" : "outline"}
            >
              {tier.name === "Enterprise" ?
                "Contact us"
              : actionLoading === tier.id ?
                "Loading..."
              : "Get started today"}
            </PrimaryCTA>
          </div>
        ))}
      </div>
    </div>
  );
}
