// import { Avatar, AvatarFallback, AvatarImage } from "@radix-ui/react-avatar";
import toast from "react-hot-toast";
import { HiMiniPhoneArrowDownLeft, HiMiniPhoneXMark } from "react-icons/hi2";
import { Button } from "./button";
import useStore, { ParticipantRole } from "@/store/store";
import { useCallback, useEffect } from "react";
import { socketService } from "@/services/socket";
import { TWebSocketMessage } from "@/payloads";
import { sounds } from "@/constants/sounds";
import { HoppAvatar } from "./hopp-avatar";
import { tauriUtils } from "@/windows/window-utils";

export const CallBanner = ({ callerId, toastId }: { callerId: string; toastId: string }) => {
  let caller = useStore((state) => state?.teammates?.find((user) => user.id === callerId));

  if (!caller) {
    // Set default caller if not found
    caller = {
      id: callerId,
      first_name: "",
      last_name: "",
      avatar_url: null,
      email: "",
      team_name: "",
      is_admin: false,
    };
  }

  const { setCallTokens } = useStore();

  const handleReject = useCallback(() => {
    sounds.incomingCall.stop();
    socketService.send({
      type: "call_reject",
      payload: {
        caller_id: callerId,
      },
    });
    toast.dismiss(toastId);
  }, [callerId, toastId]);

  const handleAnswer = useCallback(() => {
    sounds.incomingCall.stop();

    // Add a websocket listener for getting the call tokens
    // If this will not be resolved in 5 seconds we make
    // the assumption that the back-end failed to send the call tokens
    // and we reject the call with an error banner
    // We have a generic listener for call_tokens_callback
    // that will be used to store the tokens and handle the call setup
    // for users that are not with a call-banner (the callers)
    let tokensReceived = false;

    const handleCallTokens = (data: TWebSocketMessage) => {
      if (data.type === "call_tokens") {
        console.log("Received call_tokens", data);
        tokensReceived = true;
        sounds.callAccepted.play();
        tauriUtils.callStarted(callerId);

        setCallTokens({
          ...data.payload,
          timeStarted: new Date(),
          hasAudioEnabled: true,
          hasCameraEnabled: false,
          role: ParticipantRole.NONE,
          isRemoteControlEnabled: true,
          cameraTrackId: null,
        });

        toast.dismiss(toastId);
      }
    };

    socketService.on("call_tokens_callback", handleCallTokens);

    socketService.send({
      type: "call_accept",
      payload: {
        caller_id: callerId,
      },
    } as TWebSocketMessage);

    // Wait 5 seconds for tokens, otherwise show error and reject
    const timeoutId = setTimeout(() => {
      if (!tokensReceived) {
        toast.error("Failed to establish call. Please try again.", {
          duration: 4_000,
        });
        handleReject();
      }
      // Clean up the socket listener after timeout regardless of success/failure
      socketService.removeHandler("call_tokens_callback");
    }, 5000);

    // Return cleanup function
    return () => {
      clearTimeout(timeoutId);
      socketService.removeHandler("call_tokens_callback");
    };
  }, [callerId, toastId, handleReject]);

  useEffect(() => {
    sounds.incomingCall.play();

    // Auto-reject call after 60 seconds
    const timeoutId = setTimeout(() => {
      handleReject();
    }, 60_000);

    return () => {
      sounds.incomingCall.stop();
      clearTimeout(timeoutId);
    };
  }, [callerId, toastId]);

  return (
    <div className="flex flex-col items-start justify-center gap-2">
      <div className="flex flex-row gap-2">
        <HoppAvatar src={caller.avatar_url ?? undefined} firstName={caller.first_name} lastName={caller.last_name} />
        <div className="flex flex-col items-start justify-start">
          <span className="text-sm font-medium">
            {caller.first_name} {caller.last_name}
          </span>
          <span className="text-xs text-muted-foreground">Calling you...</span>
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
          <HiMiniPhoneArrowDownLeft className="size-4 min-w-4 min-h-4" /> Answer
        </Button>
      </div>
    </div>
  );
};
