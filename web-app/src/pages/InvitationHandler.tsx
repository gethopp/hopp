import { useAPI, isFetchError } from "@/hooks/useQueryClients";
import { useHoppStore } from "@/store/store";
import { LoginForm } from "./Login";
import { useNavigate, useParams } from "react-router";
import { useEffect, useRef } from "react";
import { toast } from "react-hot-toast";
import { useQueryClient } from "@tanstack/react-query";

const ErrorMessages: Record<number, string> = {
  409: "You cannot change teams while you have teammates. Please contact support for assistance.",
  404: "The invitation link is invalid or has expired. Please contact the team admin for a new invitation.",
};

const handledInvitationUuids = new Set<string>();

export function InvitationHandler() {
  const authToken = useHoppStore((state) => state.authToken);
  const { uuid } = useParams<{ uuid: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { useMutation } = useAPI();
  const { mutateAsync: changeTeam } = useMutation("post", "/api/auth/change-team/{uuid}", {
    retry: false,
  });
  const inviteProcessedRef = useRef<string | null>(null);

  useEffect(() => {
    if (!authToken || !uuid) {
      return;
    }

    if (inviteProcessedRef.current === uuid || handledInvitationUuids.has(uuid)) {
      return;
    }

    inviteProcessedRef.current = uuid;
    handledInvitationUuids.add(uuid);

    const acceptInvitation = async () => {
      try {
        const result = await changeTeam({
          params: {
            path: {
              uuid,
            },
          },
        });

        if (result?.team_name) {
          toast.success(`Successfully joined team: ${result.team_name}`);
        }

        queryClient.clear();
        navigate("/dashboard", { replace: true });
      } catch (error: unknown) {
        if (isFetchError(error)) {
          const message =
            ErrorMessages[error.response.status] || "Failed to join the team. Contact us if the problem persists.";
          toast.error(message);
        } else {
          toast.error("Failed to join the team. Contact us if the problem persists.");
        }

        queryClient.clear();
        navigate("/dashboard", { replace: true });
      }
    };

    void acceptInvitation();
  }, [authToken, changeTeam, navigate, queryClient, uuid]);

  if (!authToken) {
    return <LoginForm isInvitation={true} />;
  }

  return null;
}
