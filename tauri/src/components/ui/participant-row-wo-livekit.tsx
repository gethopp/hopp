import { components } from "@/openapi";
import clsx from "clsx";
import { Button } from "./button";
import { HiPhone, HiPhoneArrowDownLeft, HiPhoneArrowUpRight } from "react-icons/hi2";
import { socketService } from "@/services/socket";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import toast from "react-hot-toast";
import { sleep } from "@/lib/utils";
import { TInviteRequestMessage, TWebSocketMessage } from "@/payloads";
import useStore, { ParticipantRole } from "@/store/store";
import { sounds } from "@/constants/sounds";
import { usePostHog } from "posthog-js/react";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { tauriUtils } from "@/windows/window-utils";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { useAPI } from "@/services/query";
import { endCallAndWait, useEndCall, useJoinCall } from "@/lib/hooks";
import { cancelOutgoingCall, requestOutgoingCall } from "@/lib/outgoingCall";

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

export const ParticipantRow = (props: {
  user: components["schemas"]["BaseUser"];
  rooms: components["schemas"]["Room"][];
}) => {
  const posthog = usePostHog();
  const inviting = useStore((state) => state.inviting);
  const isCalling = useStore((state) => state.calling === props.user.id);
  const isInviting = useStore((state) => state.inviting === props.user.id);
  const { setInviting, callTokens, setCallTokens } = useStore((state) => state);
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
  const { mutateAsync: getRoomTokens } = useMutation("get", "/api/auth/room/{id}", undefined);

  const targetRoom = useMemo(
    // TODO: Modify CallPresence to return the room id in order to handle name conflicts
    () => props.rooms.find((r) => r.name === userPresence?.roomName),
    [props.rooms, userPresence?.roomName],
  );
  const isJoiningRoom = useRef(false);
  const endCall = useEndCall();

  const joinRoom = useCallback(async () => {
    if (!targetRoom || isJoiningRoom.current) return;

    isJoiningRoom.current = true;
    try {
      posthog.capture("user_join_room", {
        user_id: props.user.id,
        user_name: props.user.first_name,
        room_id: targetRoom.id,
        room_name: targetRoom.name,
      });

      if (callTokens) {
        await endCallAndWait(endCall);
      }

      const tokens = await getRoomTokens({
        params: { path: { id: targetRoom.id } },
      });

      if (!tokens) {
        toast.error("Error joining room");
        return;
      }

      sounds.callAccepted.play();
      const { startMic, startCamera } = await tauriUtils.getCallStartPreferences();
      setCallTokens({
        ...tokens,
        isRoomCall: true,
        timeStarted: new Date(),
        hasAudioEnabled: startMic,
        hasCameraEnabled: startCamera,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
        room: targetRoom,
        participants: [],
        isInitialisingCall: true,
        micLevel: 0,
      });

      try {
        await tauriUtils.callStarted(tokens.audioToken, tokens.videoToken);
      } catch {
        setCallTokens(null);
        toast.error("Failed to start call");
        return;
      }
      tauriUtils.showWindow("main");
    } catch (error: any) {
      if (error?.error === "trial-ended") {
        toast.error("Trial has expired, contact us if you want to extend it");
      } else {
        toast.error("Error joining room");
      }
    } finally {
      isJoiningRoom.current = false;
    }
  }, [targetRoom, callTokens, props.user, setCallTokens, endCall, getRoomTokens, posthog]);

  const inviteIdRef = useRef("");

  const joinOngoingCall = useJoinCall();

  const joinCall = useCallback(async () => {
    if (inACall || hasIncomingCall) return;

    posthog.capture("user_join_call", {
      user_id: props.user.id,
      user_name: props.user.first_name,
    });

    await joinOngoingCall(props.user.id);
  }, [props.user, inACall, hasIncomingCall, joinOngoingCall, posthog]);

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

    // The shared outgoing-call controller owns the request and the call
    // responses; this row only displays `calling` from the store.
    void requestOutgoingCall(props.user.id, props.user.first_name);
  }, [props.user, hasIncomingCall, posthog]);

  const inviteUser = useCallback(() => {
    posthog.capture("user_invite_request", {
      user_id: props.user.id,
      user_name: props.user.first_name,
    });

    if (!props.user.is_active) {
      console.log(`${props.user.first_name} is currently offline, cannot invite`);
      toast.error(`${props.user.first_name} is currently offline`);
      return;
    }

    sounds.ringing.play();
    inviteIdRef.current = crypto.randomUUID();
    setInviting(props.user.id);
    toast.success(`Inviting ${props.user.first_name}...`);
    socketService.send({
      type: "invite_request",
      payload: {
        invitee_id: props.user.id,
        invite_id: inviteIdRef.current,
      },
    } as TInviteRequestMessage);
  }, [props.user, setInviting, posthog]);

  // Listen for invite responses (invite_accept / invite_reject / callee_offline)
  const inviteCallbackIdRef = useRef<string>(`invite-response-${props.user.id}`);

  const cancelInvite = useCallback(() => {
    if (inviteIdRef.current) {
      socketService.send({
        type: "invite_cancel",
        payload: { invitee_id: props.user.id, invite_id: inviteIdRef.current },
      });
    }
    sounds.ringing.stop();
    setInviting(null);
  }, [props.user.id, setInviting]);

  useEffect(() => {
    socketService.on(inviteCallbackIdRef.current, async (data: TWebSocketMessage) => {
      if (!isInviting) return;

      switch (data.type) {
        case "invite_accept":
          if (data.payload.invitee_id !== props.user.id || data.payload.invite_id !== inviteIdRef.current) return;
          setInviting(null);
          sounds.ringing.stop();
          toast.success(`${props.user.first_name} accepted your invite`, { duration: 1500 });
          break;
        case "invite_reject":
          if (data.payload.invitee_id !== props.user.id || data.payload.invite_id !== inviteIdRef.current) return;
          setInviting(null);
          sounds.ringing.stop();
          sounds.unavailable.play();
          if (data.payload.reject_reason === "in-call") {
            toast.error(`${props.user.first_name} is already in a call`, { duration: 2500 });
          } else {
            toast.error(`${props.user.first_name} declined your invite`, { duration: 2500 });
          }
          break;
        case "callee_offline":
          if (data.payload.callee_id !== props.user.id) return;
          setInviting(null);
          sounds.ringing.stop();
          sounds.unavailable.play();
          toast.error(`${props.user.first_name} appears to be offline`, { duration: 2500 });
          break;
        case "error":
          setInviting(null);
          sounds.ringing.stop();
          toast.error(data.payload.error, { duration: 2500 });
          break;
      }
    });

    const timeoutId =
      isInviting ?
        setTimeout(() => {
          cancelInvite();
          toast.error(`${props.user.first_name} didn't respond`, { duration: 1500 });
        }, 65_000)
      : undefined;

    return () => {
      if (!isInviting) return;
      if (inviteCallbackIdRef.current) {
        socketService.removeHandler(inviteCallbackIdRef.current);
      }
      sounds.ringing.stop();
      setInviting(null);
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, [isInviting, cancelInvite, props.user.first_name, props.user.id, setInviting]);

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
        {userPresence?.roomName && !inACall ?
          <Button
            variant="gradient-white"
            onClick={joinRoom}
            disabled={!targetRoom || isJoiningRoom.current}
            className="px-2 w-auto h-7 flex flex-row items-center gap-1 text-slate-600"
          >
            <HiPhoneArrowDownLeft className="size-3" />
            Join
          </Button>
        : userPresence && !userPresence.roomName && !inACall ?
          <Button
            variant="gradient-white"
            onClick={joinCall}
            disabled={hasIncomingCall}
            className="px-2 w-auto h-7 flex flex-row items-center gap-1 text-slate-600"
          >
            <HiPhoneArrowDownLeft className="size-3" />
            Join
          </Button>
        : inACall && !userPresence ?
          <Button
            variant="gradient-white"
            onClick={() => {
              if (isInviting) {
                cancelInvite();
              } else {
                inviteUser();
              }
            }}
            disabled={hasIncomingCall || !props.user.is_active || (inviting !== null && !isInviting)}
            className={clsx(
              "px-2 w-auto h-7 flex flex-row items-center gap-1",
              !isInviting && "text-slate-600",
              isInviting && "text-red-500",
            )}
          >
            {isInviting ?
              <>
                <HiPhoneArrowUpRight className="size-3 animate-oscillate" />
                Cancel
              </>
            : <>
                <HiPhone className="size-3" />
                Invite
              </>
            }
          </Button>
        : <Button
            variant="gradient-white"
            onClick={() => {
              if (isCalling) {
                cancelOutgoingCall(props.user.id);
              } else {
                callUser();
              }
            }}
            disabled={inACall || hasIncomingCall}
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
