import { BACKEND_URLS } from "@/constants";
import { sounds } from "@/constants/sounds";
import { useAPI } from "@/services/query";
import useStore, { ParticipantRole } from "@/store/store";
import { useCallback, useMemo } from "react";
import toast from "react-hot-toast";
import { writeText, readText } from "@tauri-apps/plugin-clipboard-manager";
import { useParticipants } from "@livekit/components-react";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { Button } from "@/components/ui/button";
import { HiMiniLink } from "react-icons/hi2";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { Badge } from "@/components/ui/badge";

export const Rooms = () => {
  const { useMutation } = useAPI();
  const { callTokens, setCallTokens } = useStore();

  const { mutateAsync: getWatercoolerTokens, error } = useMutation("get", "/api/auth/watercooler", undefined);

  const handleJoinWatercooler = useCallback(async () => {
    try {
      const tokens = await getWatercoolerTokens({});
      if (!tokens) {
        toast.error("Error joining watercooler room");
        return;
      }
      sounds.callAccepted.play();
      setCallTokens({
        ...tokens,
        isRoomCall: true,
        timeStarted: new Date(),
        hasAudioEnabled: true,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
        cameraTrackId: null,
      });
    } catch (error) {
      toast.error("Error joining watercooler room");
    }
  }, [getWatercoolerTokens]);

  return (
    <div className="flex flex-col p-6 pt-3 w-full">
      {callTokens?.isRoomCall && <WatercoolerRoom />}
      {!callTokens?.isRoomCall && (
        <>
          <div className="flex flex-row gap-2 items-center mb-3">
            <h3 className="large">Rooms</h3>
            <Badge variant="outline">Beta</Badge>
          </div>
          <div className="flex flex-row gap-2 justify-between w-full min-w-full">
            <div
              className="group w-1/2 h-16 flex flex-col gap-5 p-4 border border-gray-200 rounded-md overflow-hidden shadow-xs relative"
              onClick={handleJoinWatercooler}
            >
              <span className="small text-nowrap text-ellipsis overflow-hidden">Watercooler 🚰</span>
              <div className="text-[10px] font-semibold absolute bottom-0.5 right-3 opacity-0 group-hover:opacity-100 transition-all duration-100">
                Join room →
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
};

const WatercoolerRoom = () => {
  const { useMutation } = useAPI();
  const participants = useParticipants();
  const { teammates, user } = useStore();

  const { mutateAsync: getWatercoolerAnonymous, error: errorAnonymous } = useMutation(
    "get",
    "/api/auth/watercooler/anonymous",
    undefined,
  );

  const handleInviteAnonymousUser = useCallback(async () => {
    const redirectURL = await getWatercoolerAnonymous({});
    if (!redirectURL || !redirectURL.redirect_url) {
      toast.error("Error generating link");
      return;
    }
    const link = `${BACKEND_URLS.BASE}${redirectURL.redirect_url}`;
    await writeText(link);
    toast.success("Link copied to clipboard");
  }, [getWatercoolerAnonymous]);

  // Parse participant identities and match with teammates
  const participantList = useMemo(() => {
    return participants
      .filter((participant) => !participant.identity.includes("video") && !participant.identity.includes("camera"))
      .map((participant) => {
        // Parse identity: format is "room:roomname:participantId:tracktype"
        // Extract participantId by splitting on ":" and taking the second-to-last part
        const identityParts = participant.identity.split(":");
        let participantId: string;

        if (identityParts.length >= 4) {
          // Format: "room:roomname:participantId:tracktype"
          participantId = identityParts[2] || participant.identity;
        } else {
          participantId = participant.identity;
        }

        // Handle anonymous participants
        if (participantId === "anonymous" || !participantId) {
          return {
            id: participant.identity,
            participantId: "anonymous",
            user: null,
            isLocal: participant.isLocal,
          };
        }

        // Find user in teammates or current user
        let foundUser = null;
        if (user && user.id === participantId) {
          foundUser = user;
        } else if (teammates) {
          foundUser = teammates.find((teammate) => teammate.id === participantId);
        }

        return {
          id: participant.identity,
          participantId,
          user: foundUser,
          isLocal: participant.isLocal,
        };
      });
  }, [participants, teammates, user]);

  return (
    <div className="flex flex-col w-full">
      <div className="flex flex-row gap-2 justify-between items-center mb-4">
        <div>
          <h3 className="small">Watercooler 🚰</h3>
          <span className="text-xs font-medium text-slate-600 mb-2">Participants ({participantList.length})</span>
        </div>
        <div className="flex flex-row gap-2">
          <Button
            variant="outline"
            size="icon-sm"
            onClick={() => {
              handleInviteAnonymousUser();
            }}
          >
            <TooltipProvider delayDuration={100}>
              <Tooltip>
                <TooltipTrigger>
                  <HiMiniLink className="size-3.5" />
                </TooltipTrigger>
                <TooltipContent side="left" sideOffset={10} className="flex flex-col items-center gap-0">
                  <span>Invite anonymous user</span>
                  <span className="text-xs text-slate-400">expires in 10 mins</span>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </Button>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <div className="flex flex-col gap-3">
          {participantList.map((participant) => (
            <div key={participant.id} className="flex items-center gap-3">
              {participant.user ?
                <>
                  <HoppAvatar
                    src={participant.user.avatar_url || undefined}
                    firstName={participant.user.first_name}
                    lastName={participant.user.last_name}
                    status="online"
                  />
                  <div className="flex flex-col">
                    <span className="text-sm font-medium">
                      {participant.user.first_name} {participant.user.last_name}
                      {participant.isLocal && " (You)"}
                    </span>
                  </div>
                </>
              : <>
                  <div className="w-8 h-8 rounded-full bg-slate-200 flex items-center justify-center">
                    <span className="text-xs font-medium text-slate-600">?</span>
                  </div>
                  <div className="flex flex-col">
                    <span className="text-sm font-medium text-slate-600">
                      Anonymous user
                      {participant.isLocal && " (You)"}
                    </span>
                    <span className="text-xs text-slate-500">Unknown participant</span>
                  </div>
                </>
              }
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
