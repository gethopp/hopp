import { components } from "@/openapi";
import clsx from "clsx";
import { Button } from "./button";
import { HiPhone, HiPhoneArrowDownLeft, HiPhoneArrowUpRight } from "react-icons/hi2";
import { socketService } from "@/services/socket";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import toast from "react-hot-toast";
import { sleep } from "@/lib/utils";
import { TRejectCallMessage, TCallRequestMessage, TWebSocketMessage } from "@/payloads";
import useStore, { ParticipantRole } from "@/store/store";
import { sounds } from "@/constants/sounds";
import { usePostHog } from "posthog-js/react";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { tauriUtils } from "@/windows/window-utils";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { useAPI } from "@/services/query";

const TruncatedName = ({ text, className }: { text: string; className?: string }) => {
  const textRef = useRef<HTMLDivElement>(null);
  const [isTruncated, setIsTruncated] = useState(false);

  useEffect(() => {
    const checkTruncation = () => {
      const element = textRef.current;
      if (element) {
        setIsTruncated(element.scrollWidth > element.clientWidth);
      }
    };

    checkTruncation();
    window.addEventListener("resize", checkTruncation);
    return () => window.removeEventListener("resize", checkTruncation);
  }, [text]);

  const content = (
    <div ref={textRef} className={clsx("truncate cursor-default", className)}>
      {text}
    </div>
  );

  if (!isTruncated) return content;

  return (
    <TooltipProvider>
      <Tooltip delayDuration={100}>
        <TooltipTrigger asChild>{content}</TooltipTrigger>
        <TooltipContent side="top">
          <p>{text}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
};

export const ParticipantRow = (props: { user: components["schemas"]["BaseUser"] }) => {
  const posthog = usePostHog();
  const isCalling = useStore((state) => state.calling === props.user.id);
  const { setCalling, setCallTokens } = useStore((state) => state);
  const inACall = useStore((state) => state.callTokens !== null);
  const hasIncomingCall = useStore((state) => state.incomingCallCallerId !== null);
  const callsPresence = useStore((state) => state.callsPresence);
  const teammates = useStore((state) => state.teammates);
  const currentUser = useStore((state) => state.user);

  const userPresence = callsPresence?.[props.user.id];

  const callPeers = useMemo(() => {
    const ids = userPresence?.peerIds;
    if (!ids || ids.length === 0) return [];
    return ids
      .map((id) => {
        if (currentUser?.id === id) return currentUser;
        return teammates?.find((t) => t.id === id) ?? null;
      })
      .filter(Boolean);
  }, [userPresence?.peerIds, teammates, currentUser]);

  const { useMutation } = useAPI();
  const { mutateAsync: joinCallRequest } = useMutation("post", "/api/auth/call/join/{userId}", undefined);

  const callbackIdRef = useRef<string>(`call-response-${props.user.id}`);
  const callResolvedRef = useRef(false);

  const joinCall = useCallback(async () => {
    if (inACall || hasIncomingCall) return;

    posthog.capture("user_join_call", {
      user_id: props.user.id,
      user_name: props.user.first_name,
    });

    try {
      const tokens = await joinCallRequest({ params: { path: { userId: props.user.id } } });

      if (!tokens) {
        toast.error("Error joining call");
        return;
      }

      sounds.callAccepted.play();
      let startMic = false;
      let startCamera = false;
      try {
        const settings = await tauriUtils.getUserSettings();
        startMic = settings.start_mic_on_call;
        startCamera = settings.start_camera_on_call;
      } catch {
        // fall back to safe defaults
      }
      setCallTokens({
        ...tokens,
        timeStarted: new Date(),
        hasAudioEnabled: startMic,
        hasCameraEnabled: startCamera,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
        participants: [],
        isInitialisingCall: true,
        micLevel: 0,
      });
      try {
        await tauriUtils.callStarted(tokens.audioToken, tokens.videoToken);
      } catch {
        setCallTokens(null);
        return;
      }
      tauriUtils.showWindow("main");
    } catch (error: any) {
      if (error?.error === "trial-ended") {
        toast.error("Trial has expired, contact us if you want to extend it");
      } else {
        toast.error("Error joining call");
      }
    }
  }, [props.user, inACall, hasIncomingCall, joinCallRequest, setCallTokens]);

  const callUser = useCallback(() => {
    if (hasIncomingCall) return;

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

    callResolvedRef.current = false;
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
  }, [props.user, hasIncomingCall]);

  // Add a useEffect to listen for call responses
  // that will unsubscribe from the socket when the component unmounts
  useEffect(() => {
    // Add listener for call response
    socketService.on(callbackIdRef.current, async (data: TWebSocketMessage) => {
      if (!isCalling) return;

      switch (data.type) {
        case "call_reject": {
          const { payload } = data as TRejectCallMessage;
          if (payload.reject_reason == "in-call") {
            toast.error(`${props.user.first_name} is already in a call`, {
              duration: 2500,
            });
          } else if (payload.reject_reason === "trial-ended") {
            toast.error(`Trial has expired, contact us if you want to extend it`, {
              duration: 2500,
            });
          } else if (payload.reject_reason === "already-calling") {
            toast.error(`Already calling ${props.user.first_name}, please wait`, {
              duration: 2500,
            });
          } else {
            toast.error(`${props.user.first_name} rejected your call`, {
              duration: 2500,
            });
          }
          callResolvedRef.current = true;
          setCalling(null);
          sounds.ringing.stop();
          sounds.unavailable.play();
          break;
        }
        case "callee_offline":
          toast.error(`${props.user.first_name} appears to be offline`, {
            duration: 2500,
          });
          callResolvedRef.current = true;
          setCalling(null);
          sounds.ringing.stop();
          sounds.unavailable.play();
          break;
        case "call_accept":
          toast.success(`${props.user.first_name} accepted your call`, {
            duration: 1500,
          });
          break;
        case "call_tokens": {
          callResolvedRef.current = true;
          setCalling(null);
          sounds.ringing.stop();
          sounds.callAccepted.play();
          tauriUtils.showWindow("main");
          let startMic = false;
          let startCamera = false;
          try {
            const settings = await tauriUtils.getUserSettings();
            startMic = settings.start_mic_on_call;
            startCamera = settings.start_camera_on_call;
          } catch {
            // fall back to safe defaults
          }
          setCallTokens({
            ...data.payload,
            timeStarted: new Date(),
            hasAudioEnabled: startMic,
            hasCameraEnabled: startCamera,
            role: ParticipantRole.NONE,
            isRemoteControlEnabled: true,
            participants: [],
            isInitialisingCall: true,
            micLevel: 0,
          });
          try {
            await tauriUtils.callStarted(data.payload.audioToken, data.payload.videoToken);
          } catch {
            setCallTokens(null);
          }
          break;
        }
      }
    });

    const timeoutId =
      isCalling ?
        setTimeout(
          () => {
            callResolvedRef.current = true;
            sounds.ringing.stop();
            setCalling(null);
            socketService.send({
              type: "call_end",
              payload: { participant_id: props.user.id },
            });
            toast.error(`${props.user.first_name} didn't answer`, { duration: 1500 });
          },
          // 5 secs more from auto-reject from callee's timeout
          65_000,
        )
      : undefined;

    return () => {
      if (!isCalling) return;
      console.debug("Unsubscribing from call response for user:", props.user.id);
      if (callbackIdRef.current) {
        socketService.removeHandler(callbackIdRef.current);
      }
      sounds.ringing.stop();
      setCalling(null);
      if (!callResolvedRef.current) {
        socketService.send({
          type: "call_end",
          payload: { participant_id: props.user.id },
        });
      }
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, [isCalling]);

  return (
    <div className="grid grid-cols-[max-content_minmax(0,1fr)_max-content] gap-2 w-full items-center">
      <HoppAvatar
        src={props.user.avatar_url || undefined}
        firstName={props.user.first_name}
        lastName={props.user.last_name}
        status={props.user.is_active ? "online" : "offline"}
        callPeers={callPeers.map((p) => ({
          avatarUrl: p?.avatar_url || undefined,
          firstName: p?.first_name ?? "",
          lastName: p?.last_name ?? "",
        }))}
      />

      <div className="flex flex-col justify-center h-10 overflow-hidden">
        <TruncatedName text={`${props.user.first_name} ${props.user.last_name}`} className="medium" />

        <div className="muted truncate text-xs text-slate-500">
          {userPresence ?
            userPresence.roomName ?
              `In ${userPresence.roomName}`
            : "In a call"
          : props.user.is_active ?
            "Online"
          : "Offline"}
        </div>
      </div>

      <div className="mr-4">
        {userPresence && !userPresence.roomName && !inACall ?
          <Button
            variant="gradient-white"
            onClick={joinCall}
            disabled={hasIncomingCall}
            className="px-2 w-auto h-7 flex flex-row items-center gap-1 text-slate-600"
          >
            <HiPhoneArrowDownLeft className="size-3" />
            Join
          </Button>
        : <Button
            variant="gradient-white"
            onClick={() => {
              if (isCalling) {
                callResolvedRef.current = true;
                sounds.ringing.stop();
                setCalling(null);
                socketService.send({
                  type: "call_end",
                  payload: { participant_id: props.user.id },
                });
              } else {
                callUser();
              }
            }}
            disabled={inACall || hasIncomingCall || !!userPresence}
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
        }
      </div>
    </div>
  );
};
