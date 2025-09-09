import { useEffect } from "react";
import { useAPI } from "@/hooks/useQueryClients";

// Extend the Window interface to include TallyConfig
declare global {
  interface Window {
    TallyConfig?: {
      formId: string;
      popup: {
        width: number;
        layout: string;
        autoClose: number;
        showOnce: boolean;
        doNotShowAfterSubmit: boolean;
        hiddenFields?: Record<string, string>;
      };
    };
  }
}

export function SignInSuccessModal() {
  const { useQuery, useMutation } = useAPI();

  const { data: user } = useQuery("get", "/api/auth/user", undefined, {
    select: (data) => data,
  });

  const { mutateAsync: updateOnboardingFormStatus } = useMutation("post", "/api/auth/metadata/onboarding-form");

  const hasFilledForm = user?.metadata?.hasFilledOnboardingForm || false;

  useEffect(() => {
    if (import.meta.env.MODE === "development") return;

    if (user && !hasFilledForm) {
      // Load Tally script if not already loaded
      if (!document.querySelector('script[src="https://tally.so/widgets/embed.js"]')) {
        const script = document.createElement("script");
        script.src = "https://tally.so/widgets/embed.js";
        script.async = true;
        document.head.appendChild(script);
      }

      // Configure Tally with user's email as hidden field
      window.TallyConfig = {
        formId: "nPeOk0",
        popup: {
          width: 700,
          layout: "modal",
          autoClose: 0,
          showOnce: false,
          doNotShowAfterSubmit: true,
          hiddenFields: {
            email: user.email,
          },
        },
      };

      // Set up form submission handler to update onboarding status
      const handleFormSubmit = async () => {
        await updateOnboardingFormStatus({});
      };

      const messageHandler = (event: MessageEvent) => {
        // Odd way to check but cool:
        // https://tally.so/help/developer-resources#b4334b72c8424397a0e4fc2098d54c07
        if (event?.data?.includes("Tally.FormSubmitted")) {
          handleFormSubmit();
        }
      };

      window.addEventListener("message", messageHandler);

      return () => {
        window.removeEventListener("message", messageHandler);
      };
    }
  }, [user, hasFilledForm, updateOnboardingFormStatus]);

  if (!user || hasFilledForm) {
    return null;
  }

  return null;
}
