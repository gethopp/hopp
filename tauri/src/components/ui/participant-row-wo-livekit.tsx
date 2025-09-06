import { components } from "@/openapi";
import clsx from "clsx";
import { Button } from "./button";
import { HiPhone, HiPhoneArrowUpRight } from "react-icons/hi2";
import { socketService } from "@/services/socket";
import { useCallback, useEffect, useRef, useState } from "react";
import toast from "react-hot-toast";
import { sleep } from "@/lib/utils";
import { TCallRequestMessage, TWebSocketMessage } from "@/payloads";
import useStore, { ParticipantRole } from "@/store/store";
import { sounds } from "@/constants/sounds";
import { usePostHog } from "posthog-js/react";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { tauriUtils } from "@/windows/window-utils";

export const ParticipantRow = (props: { user: components["schemas"]["BaseUser"] }) => {
  const posthog = usePostHog();
  const isCalling = useStore((state) => state.calling === props.user.id);
  const { setCalling, setCallTokens } = useStore((state) => state);
  const inACall = useStore((state) => state.callTokens !== null);

  const callbackIdRef = useRef<string>(`call-response-${props.user.id}`);

  const callUser = useCallback(() => {
    posthog.capture("user_call_request", {
      user_id: props.user.id,
      user_name: props.user.first_name,
    });
    // TODO: Send event even is user is offline
    // to avoid skipping calls that may the user is online
    // and we have bad caching
    if (!props.user.is_active) {
      console.log(`${props.user.first_name} is currently offline, playing failure sound`);

      setCalling(null);
      const playThreeTimes = async () => {
        for (let i = 0; i < 3; i++) {
          sounds.unavailable.play();
          if (i < 2) await sleep(1000);
        }
      };

      playThreeTimes();
      toast.remove();
      toast.error(`${props.user.first_name} is currently offline`);
      return;
    }

    sounds.ringing.play();
    setCalling(props.user.id);

    toast.success(`Calling ${props.user.first_name}...`);
    // Send call request
    socketService.send({
      type: "call_request",
      payload: {
        callee_id: props.user.id,
      },
    } as TCallRequestMessage);
  }, [props.user]);

  // Add a useEffect to listen for call responses
  // that will unsubscribe from the socket when the component unmounts
  useEffect(() => {
    // Add listener for call response
    socketService.on(callbackIdRef.current, (data: TWebSocketMessage) => {
      if (!isCalling) return;

      switch (data.type) {
        case "call_reject":
          toast.error(`${props.user.first_name} rejected your call`, {
            duration: 2500,
          });
          setCalling(null);
          sounds.ringing.stop();
          sounds.unavailable.play();
          break;
        case "callee_offline":
          toast.error(`${props.user.first_name} appears to be offline`, {
            duration: 2500,
          });
          setCalling(null);
          sounds.ringing.stop();
          sounds.unavailable.play();
          break;
        case "call_accept":
          toast.success(`${props.user.first_name} accepted your call`, {
            duration: 1500,
          });
          break;
        case "call_tokens":
          setCalling(null);
          sounds.ringing.stop();
          sounds.callAccepted.play();
          tauriUtils.showMainWindow();
          setCallTokens({
            ...data.payload,
            timeStarted: new Date(),
            hasAudioEnabled: true,
            hasCameraEnabled: false,
            role: ParticipantRole.NONE,
            isRemoteControlEnabled: true,
          });
          break;
      }
    });

    return () => {
      if (!isCalling) return;

      console.debug("Unsubscribing from call response for user:", props.user.id);
      if (callbackIdRef.current) {
        socketService.removeHandler(callbackIdRef.current);
      }
      // Stop any playing sounds when component unmounts
      sounds.ringing.stop();
      setCalling(null);
    };
  }, [isCalling]);

  return (
    <div className="flex flex-row gap-2 w-full items-center">
      <HoppAvatar
        src={props.user.avatar_url || undefined}
        firstName={props.user.first_name}
        lastName={props.user.last_name}
        status={props.user.is_active ? "online" : "offline"}
      />
      <div
        className="h-10 flex flex-col w-full"
        style={{
          maxWidth: "calc(100% - 90px)",
        }}
      >
        <span className="medium whitespace-nowrap overflow-hidden text-ellipsis">
          {props.user.first_name} {props.user.last_name}
        </span>
        <span className="muted text-xs text-slate-500">{props.user.is_active ? "Online" : "Offline"}</span>
      </div>
      <div className="ml-auto mr-4">
        <Button
          variant="gradient-white"
          onClick={() => {
            if (isCalling) {
              sounds.ringing.stop();
              setCalling(null);
            } else {
              callUser();
            }
          }}
          disabled={inACall}
          className={clsx(
            "px-2 w-auto h-7 flex flex-row items-center gap-1",
            !isCalling && "text-slate-600",
            isCalling && "text-red-500",
          )}
        >
          {isCalling ?
            <>
              <HiPhoneArrowUpRight className="size-3 animate-oscillate" />
              End
            </>
          : <>
              <HiPhone className="size-3" />
              Call
            </>
          }
        </Button>
      </div>
    </div>
  );
};
