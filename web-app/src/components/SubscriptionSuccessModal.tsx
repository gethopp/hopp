import { useEffect } from "react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { CheckCircle } from "lucide-react";
import confetti from "canvas-confetti";
import { useAPI } from "@/hooks/useQueryClients";
import { useHoppStore } from "@/store/store";

interface SubscriptionSuccessModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SubscriptionSuccessModal({ open, onOpenChange }: SubscriptionSuccessModalProps) {
  const { authToken } = useHoppStore();
  const { useQuery } = useAPI();

  // Determine whether this success is a brand-new trial or a re-subscribe so we
  // can tailor the copy. A card-on-file trial returns `trialing`; re-subscribing
  // an existing team charges immediately and returns `active`.
  const { data: subscriptionData } = useQuery("get", "/api/auth/billing/subscription", undefined, {
    queryHash: `subscription-${authToken}`,
    enabled: open,
  });

  const isTrial = subscriptionData?.subscription?.status === "trialing";

  const title = isTrial ? "🚢 Welcome onboard to Hopp" : "🎉 Welcome back to Hopp";
  const subtitle =
    isTrial ?
      "Your trial to Hopp just started. Enjoy all the premium features for 14 days."
    : "We are excited to have you back. You can now start pairing again.";

  useEffect(() => {
    if (!open) return;

    // Trigger confetti animation
    const triggerConfetti = () => {
      // Create multiple bursts of confetti
      confetti({
        particleCount: 100,
        spread: 70,
        origin: { y: 0.6 },
      });

      setTimeout(() => {
        confetti({
          particleCount: 50,
          spread: 60,
          origin: { x: 0.2, y: 0.6 },
        });
      }, 200);

      setTimeout(() => {
        confetti({
          particleCount: 50,
          spread: 60,
          origin: { x: 0.8, y: 0.6 },
        });
      }, 400);
    };

    triggerConfetti();
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader className="text-center">
          <div className="mx-auto mb-2 w-16 h-16 bg-green-100 rounded-full flex items-center justify-center">
            <CheckCircle className="w-8 h-8 text-green-600" />
          </div>
          <DialogTitle className="text-2xl font-bold text-gray-900">{title}</DialogTitle>
          <DialogDescription className="text-lg text-gray-600 mt-2">{subtitle}</DialogDescription>
        </DialogHeader>

        <div className="mt-1 space-y-3">
          <div className="flex items-center text-sm text-gray-600">
            <CheckCircle className="w-4 h-4 text-green-500 mr-2" />
            Unlimited calls
          </div>
          <div className="flex items-center text-sm text-gray-600">
            <CheckCircle className="w-4 h-4 text-green-500 mr-2" />
            Priority support
          </div>
          <div className="flex items-center text-sm text-gray-600">
            <CheckCircle className="w-4 h-4 text-green-500 mr-2" />
            <span>
              Supporting OSS and
              <a
                href="https://gethopp.app/about"
                target="_blank"
                rel="noopener noreferrer"
                className="link no-underline"
              >
                {" "}
                our small team
              </a>
            </span>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
