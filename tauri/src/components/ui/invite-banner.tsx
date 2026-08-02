import toast from "react-hot-toast";
import { HiMiniPhoneArrowDownLeft, HiMiniPhoneXMark } from "react-icons/hi2";
import { Button } from "./button";
import useStore from "@/store/store";
import { useCallback, useEffect, useMemo } from "react";
import { socketService } from "@/services/socket";
import { sounds } from "@/constants/sounds";
import { HoppAvatar } from "./hopp-avatar";
import throttle from "lodash/throttle";
import { useJoinCall } from "@/lib/hooks";
import { TWebSocketMessage } from "@/payloads";

const ACTION_THROTTLE_MS = 6000;

export const InviteBanner = ({
  inviterId,
  inviteId,
  toastId,
}: {
  inviterId: string;
  inviteId: string;
  toastId: string;
}) => {
  let inviter = useStore((state) => state?.teammates?.find((user) => user.id === inviterId));

  if (!inviter) {
    inviter = {
      id: inviterId,
      first_name: "",
      last_name: "",
      avatar_url: null,
      email: "",
      team_name: "",
      is_admin: false,
    };
  }

  const { setIncomingInviteInviterId } = useStore();
  const joinOngoingCall = useJoinCall();

  const handleReject = useMemo(
    () =>
      throttle(
        () => {
          sounds.incomingCall.stop();
          socketService.send({
            type: "invite_reject",
            payload: {
              inviter_id: inviterId,
              invite_id: inviteId,
            },
          });
          toast.dismiss(toastId);
          setIncomingInviteInviterId(null);
        },
        ACTION_THROTTLE_MS,
        { leading: true, trailing: false },
      ),
    [inviterId, inviteId, toastId, setIncomingInviteInviterId],
  );

  const dismissInvite = useCallback(() => {
    toast.dismiss(toastId);
    setIncomingInviteInviterId(null);
  }, [toastId, setIncomingInviteInviterId]);

  const handleAnswer = useMemo(
    () =>
      throttle(
        async () => {
          sounds.incomingCall.stop();

          const joined = await joinOngoingCall(inviterId);
          if (joined) {
            socketService.send({
              type: "invite_accept",
              payload: { inviter_id: inviterId, invite_id: inviteId },
            });
          } else {
            socketService.send({
              type: "invite_reject",
              payload: { inviter_id: inviterId, invite_id: inviteId },
            });
          }
          dismissInvite();
        },
        ACTION_THROTTLE_MS,
        { leading: true, trailing: false },
      ),
    [inviterId, inviteId, joinOngoingCall, dismissInvite],
  );

  useEffect(() => {
    sounds.incomingCall.play();

    // Auto-reject invite after 60 seconds
    const timeoutId = setTimeout(() => {
      handleReject();
    }, 60_000);

    return () => {
      sounds.incomingCall.stop();
      clearTimeout(timeoutId);
    };
  }, [inviterId, toastId]);

  useEffect(() => {
    const handlerId = `invite-cancel-${inviteId}`;
    socketService.on(handlerId, (data: TWebSocketMessage) => {
      if (data.type !== "invite_cancel" || data.payload.inviter_id !== inviterId || data.payload.invite_id !== inviteId)
        return;
      sounds.incomingCall.stop();
      toast.dismiss(toastId);
      setIncomingInviteInviterId(null);
    });
    return () => socketService.removeHandler(handlerId);
  }, [inviterId, inviteId, toastId, setIncomingInviteInviterId]);

  useEffect(() => {
    return () => {
      handleReject.cancel();
      handleAnswer.cancel();
    };
  }, [handleReject, handleAnswer]);

  return (
    <div className="flex flex-col items-start justify-center gap-2">
      <div className="flex flex-row gap-2">
        <HoppAvatar src={inviter.avatar_url ?? undefined} firstName={inviter.first_name} lastName={inviter.last_name} />
        <div className="flex flex-col items-start justify-start">
          <span className="text-sm font-medium">
            {inviter.first_name} {inviter.last_name}
          </span>
          <span className="text-xs text-muted-foreground">Invited you to join a call</span>
        </div>
      </div>
      <div className="flex flex-row gap-1">
        <Button variant="ghost" size="sm" onClick={handleReject} className="hover:bg-red-100 flex flex-row gap-2">
          <HiMiniPhoneXMark className="size-4" /> Not now
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={handleAnswer}
          className="btn-gradient-white hover:scale-[1.025] transition-all duration-200 flex flex-row gap-2 px-4 hover:text-green-700"
        >
          <HiMiniPhoneArrowDownLeft className="size-4 min-w-4 min-h-4" /> Join
        </Button>
      </div>
    </div>
  );
};
