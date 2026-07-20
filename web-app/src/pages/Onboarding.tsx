import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useForm } from "@tanstack/react-form";
import { useAPI } from "@/hooks/useQueryClients";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toast } from "react-hot-toast";
import { CreditCard, ShieldCheck } from "lucide-react";
import Logo from "@/assets/Hopp.png";
import { HiMiniCheck, HiOutlineQuestionMarkCircle } from "react-icons/hi2";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

import { PairingToolMultiSelect, ReferralSourceSelect } from "@/components/onboarding-fields";

const TRIAL_DAYS = 14;

// Drafts are scoped per account so one admin's onboarding answers can't leak
// into another account signed in on the same browser.
const ONBOARDING_DRAFT_PREFIX = "hopp:onboarding-draft";

function draftKey(userId: string): string {
  return `${ONBOARDING_DRAFT_PREFIX}:${userId}`;
}

interface OnboardingFormValues {
  teamName: string;
  pairingTool: string[];
  hearAboutHopp: string;
  hearAboutHoppOther: string;
}

function loadDraft(userId: string): Partial<OnboardingFormValues> {
  try {
    const stored = localStorage.getItem(draftKey(userId));
    if (!stored) return {};
    const parsed = JSON.parse(stored);
    return typeof parsed === "object" && parsed !== null ? (parsed as Partial<OnboardingFormValues>) : {};
  } catch {
    return {};
  }
}

function clearDraft(userId: string) {
  localStorage.removeItem(draftKey(userId));
}

export function Onboarding() {
  const { useMutation, useQuery } = useAPI();
  const navigate = useNavigate();
  const [step, setStep] = useState<1 | 2>(1);
  const [showValidationErrors, setShowValidationErrors] = useState(false);
  const [checkoutLoading, setCheckoutLoading] = useState(false);

  // Reverse guard: users who don't need to add a card (subscribed teams,
  // non-admins, invited users, pre-cutoff teams) shouldn't be stuck here.
  const { data: subscriptionData } = useQuery("get", "/api/auth/billing/subscription", undefined, {});
  const subscription = subscriptionData?.subscription;
  const needsOnboarding = !!subscription?.requires_payment_method && !!subscription?.is_admin;

  // The current account id scopes the localStorage draft to this user.
  const { data: currentUser } = useQuery("get", "/api/auth/user", undefined, {});
  const userId = currentUser?.id;

  useEffect(() => {
    if (subscription && !needsOnboarding) {
      if (userId) clearDraft(userId);
      navigate("/dashboard", { replace: true });
    }
  }, [subscription, needsOnboarding, navigate, userId]);

  const { mutateAsync: updateTeam } = useMutation("patch", "/api/auth/team");
  const { mutateAsync: updateOnboardingFormStatus } = useMutation("post", "/api/auth/metadata/onboarding-form");
  const createCheckoutSessionMutation = useMutation("post", "/api/auth/billing/create-checkout-session");

  const form = useForm({
    defaultValues: {
      teamName: "",
      pairingTool: [] as string[],
      hearAboutHopp: "",
      hearAboutHoppOther: "",
    },
    onSubmit: async ({ value }: { value: OnboardingFormValues }) => {
      const teamName = value.teamName.trim();
      if (
        teamName === "" ||
        value.pairingTool.length === 0 ||
        value.hearAboutHopp === "" ||
        (value.hearAboutHopp === "other" && value.hearAboutHoppOther === "")
      ) {
        setShowValidationErrors(true);
        toast.error("Please fill in all required fields");
        return;
      }

      try {
        await updateTeam({ body: { name: teamName } });
        await updateOnboardingFormStatus({
          body: {
            onboarding: {
              pairingTool: value.pairingTool,
              hearAboutHopp: value.hearAboutHopp,
              hearAboutHoppOther: value.hearAboutHoppOther,
            },
          },
        });
        setStep(2);
      } catch (error) {
        console.error(error);
        toast.error("Something went wrong saving your details. Please try again.");
      }
    },
  });

  // Load the per-account draft once the current user is known. Deferred (instead
  // of in defaultValues) because the user id resolves asynchronously.
  const draftLoadedRef = useRef(false);
  useEffect(() => {
    if (!userId || draftLoadedRef.current) return;
    draftLoadedRef.current = true;
    const draft = loadDraft(userId);
    if (Object.keys(draft).length > 0) {
      form.reset({ ...form.state.values, ...draft });
    }
  }, [userId, form]);

  useEffect(() => {
    if (!userId) return;
    const subscription = form.store.subscribe(() => {
      localStorage.setItem(draftKey(userId), JSON.stringify(form.store.state.values));
    });
    return () => subscription.unsubscribe();
  }, [form, userId]);

  const handleStartTrial = async () => {
    setCheckoutLoading(true);
    try {
      const response = await createCheckoutSessionMutation.mutateAsync({
        body: {
          tier: "paid" as const,
          interval: "monthly" as const,
        },
      });

      if (response?.checkout_url) {
        window.location.href = response.checkout_url;
      }
    } catch (error: unknown) {
      console.error("Error creating checkout session:", error);
      const message = error instanceof Error ? error.message : "Failed to start your trial";
      toast.error(message);
      setCheckoutLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen w-screen flex-col items-center justify-center bg-linear-to-b from-[#F5F0FF] via-white to-white p-4">
      <div className="w-full max-w-2xl">
        <img src={Logo} alt="Hopp" className="mx-auto mb-8 h-10 w-auto" />

        {/* Step indicator */}
        <div className="mb-8 flex items-center justify-center gap-2">
          <StepDot active={step === 1} done={step > 1} label="Your team" />
          <div className="h-px w-10 bg-slate-300" />
          <StepDot active={step === 2} done={false} label="Payment" />
        </div>

        <div className="rounded-2xl border border-slate-200 bg-white p-8 shadow-xl ring-2 ring-white">
          {step === 1 ?
            <div className="flex flex-col gap-4">
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  form.handleSubmit();
                }}
                className="space-y-6"
              >
                <div>
                  <h1 className="text-2xl font-semibold">Tell us about your team</h1>
                  <p className="mt-2 text-base font-normal text-gray-600">
                    A few quick questions so we can set up your workspace.
                  </p>
                </div>

                <form.Field name="teamName">
                  {(field) => {
                    const hasError = showValidationErrors && field.state.value.trim() === "";
                    return (
                      <div className="space-y-2">
                        <Label htmlFor="teamName">
                          Team name <span className="text-red-500">*</span>
                        </Label>
                        <Input
                          id="teamName"
                          value={field.state.value}
                          onChange={(e) => field.handleChange(e.target.value)}
                          placeholder="Acme Engineering"
                        />
                        {hasError && <p className="text-sm text-red-500">Team name is required</p>}
                      </div>
                    );
                  }}
                </form.Field>

                <ReferralSourceSelect form={form} showErrors={showValidationErrors} />
                <PairingToolMultiSelect form={form} showErrors={showValidationErrors} />

                <form.Subscribe selector={(state) => ({ isSubmitting: state.isSubmitting })}>
                  {({ isSubmitting }: { isSubmitting: boolean }) => (
                    <div className="flex justify-end pt-2">
                      <Button type="submit" disabled={isSubmitting}>
                        {isSubmitting ? "Saving..." : "Continue"}
                      </Button>
                    </div>
                  )}
                </form.Subscribe>
              </form>

              <div className="flex items-center gap-4 mx-6">
                <span className="h-px flex-1 bg-slate-200" />
                <span className="text-xs font-medium text-slate-400">Or</span>
                <span className="h-px flex-1 bg-slate-200" />
              </div>

              <div className="space-y-1 text-center">
                <h3 className="text-md font-semibold">Want to join an existing team?</h3>
                <p className="text-sm text-muted-foreground">You'll need an invite link from a teammate.</p>
              </div>
            </div>
          : <div className="space-y-6">
              <div>
                <h1 className="text-2xl font-semibold">Start your {TRIAL_DAYS}-day free trial</h1>
                <p className="mt-2 text-base font-normal text-gray-600">
                  Add a payment method to unlock Hopp. You won't be charged today and your card is only billed when the{" "}
                  {TRIAL_DAYS}-day trial ends, and you can cancel anytime before then with one click.
                </p>
                <p className="mt-3 text-base font-bold text-gray-900">
                  Only one team member needs to add a credit card. After that, you can invite everyone else, and they
                  can join with the invite link without having to add a credit card.
                </p>
                <TooltipProvider delayDuration={0.2}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        className="mt-3 inline-flex items-center gap-1.5 text-sm font-normal text-indigo-600 hover:text-indigo-700"
                      >
                        <HiOutlineQuestionMarkCircle className="size-5 stroke-2" />
                        Why do you require a credit card?
                      </button>
                    </TooltipTrigger>
                    <TooltipContent className="max-w-xs space-y-2 text-left leading-relaxed">
                      <p>
                        We require a credit card to avoid the noise of people who don't want to commit using the hosted
                        Hopp solution.
                      </p>
                      <p className="mb-0">
                        We have costs for media sharing, and we work on this nights and weekends, so we want to spend
                        all our bandwidth on teams that care most about pairing.
                      </p>
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>

              <ul className="space-y-3 text-sm text-gray-700">
                <li className="flex items-start gap-3">
                  <ShieldCheck className="mt-0.5 size-5 shrink-0 text-indigo-600" />
                  No charge for {TRIAL_DAYS} days, cancel anytime
                </li>
                <li className="flex items-start gap-3">
                  <CreditCard className="mt-0.5 size-5 shrink-0 text-indigo-600" />
                  Secure card collection handled by Stripe
                </li>
              </ul>

              <div className="flex flex-col gap-3 sm:flex-row sm:justify-between">
                <Button type="button" variant="ghost" onClick={() => setStep(1)} disabled={checkoutLoading}>
                  Back
                </Button>
                <Button type="button" onClick={handleStartTrial} disabled={checkoutLoading}>
                  {checkoutLoading ? "Redirecting to Stripe" : "Add payment method"}
                </Button>
              </div>
            </div>
          }
        </div>
      </div>
    </div>
  );
}

function StepDot({ active, done, label }: { active: boolean; done: boolean; label: string }) {
  return (
    <div className="flex items-center gap-2">
      <span
        className={
          "flex size-6 items-center justify-center rounded-full text-xs font-medium " +
          (active || done ? "bg-indigo-600 text-white" : "bg-slate-200 text-slate-500")
        }
      >
        {done ?
          <HiMiniCheck className="size-4" />
        : label === "Your team" ?
          "1"
        : "2"}
      </span>
      <span className={"text-sm " + (active ? "font-medium text-slate-900" : "text-slate-500")}>{label}</span>
    </div>
  );
}
