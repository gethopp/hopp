import { useState, useEffect } from "react";
import { useAPI } from "@/hooks/useQueryClients";
import { OnboardingModal } from "@/components/OnboardingModal";

export function SignInSuccessModal() {
  const { useQuery } = useAPI();
  const [isOpen, setIsOpen] = useState(false);

  const { data: user } = useQuery("get", "/api/auth/user", undefined, {
    select: (data) => data,
  });

  const { data: teammates } = useQuery("get", "/api/auth/teammates", undefined, {
    select: (data) => data,
  });

  const hasFilledForm = user?.metadata?.hasFilledOnboardingForm || false;
  const hasNoTeammates = teammates?.length === 0;

  useEffect(() => {
    // Only show onboarding modal if:
    // 1. User is loaded
    // 2. User hasn't filled the form
    // 3. User has no teammates (admin sign up)
    if (user && !hasFilledForm && hasNoTeammates) {
      setIsOpen(true);
    }
  }, [user, hasFilledForm, hasNoTeammates]);

  if (!user || hasFilledForm || !hasNoTeammates) {
    return null;
  }

  return <OnboardingModal open={isOpen} onOpenChange={setIsOpen} />;
}
