import { CopiableInput } from "@/components/ui/copiable-input";
import { useBackendBaseUrl } from "@/constants";
import { useAPI } from "@/services/query";
import { usePostHog } from "posthog-js/react";
import headerBackground from "../../assets/header-invite.png";

function Invite() {
  const posthog = usePostHog();
  const { useQuery } = useAPI();
  const backendBaseUrl = useBackendBaseUrl();

  const { data: inviteData, error } = useQuery("get", "/api/auth/get-invite-uuid", undefined, {
    queryHash: `invite`,
    refetchInterval: 10000,
    select: (data) => data,
  });

  return (
    <div className="flex flex-col items-start justify-center h-full p-4 gap-0 relative">
      <img
        src={headerBackground}
        alt="header-background"
        className="select-none absolute -top-4 left-0 w-full h-auto"
        draggable="false"
      />
      <h4
        style={{
          background: "linear-gradient(180deg, #000000 0%, #636363 180.67%)",
          WebkitBackgroundClip: "text",
          WebkitTextFillColor: "transparent",
          backgroundClip: "text",
        }}
        className="small mt-[80px] mb-2 text-center w-full"
      >
        Pairing is better with your friends
      </h4>
      <span className="muted text-center w-full">
        Invite your friends to join your team and start pairing together.
      </span>
      {error && <span className="muted text-red-500 mt-4">Error fetching invitation URL</span>}
      {inviteData && (
        <div className="w-full">
          <CopiableInput
            value={inviteData?.invite_uuid ? `${backendBaseUrl}/invitation/${inviteData.invite_uuid}` : ""}
            readOnly
            className="text-slate-600 mt-4 w-full"
            onCopy={() => {
              // Send posthog event for custom URL sign-in
              posthog.capture("user_click_app_invitation");
            }}
          />
        </div>
      )}
    </div>
  );
}

export default Invite;
