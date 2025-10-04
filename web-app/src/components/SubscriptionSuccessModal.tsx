import { useEffect } from "react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { CheckCircle } from "lucide-react";
import confetti from "canvas-confetti";

interface SubscriptionSuccessModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SubscriptionSuccessModal({ open, onOpenChange }: SubscriptionSuccessModalProps) {
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
          <DialogTitle className="text-2xl font-bold text-gray-900">🎉 Welcome to Hopp Pro!</DialogTitle>
          <DialogDescription className="text-lg text-gray-600 mt-2">
            You are now subscribed to Hopp Pro. Enjoy all the premium features!
          </DialogDescription>
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
