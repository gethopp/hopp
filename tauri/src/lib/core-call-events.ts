import { listen } from "@tauri-apps/api/event";
import useStore from "@/store/store";

export function listenForCoreCallEnd(endCall: (callId: string) => void) {
  return listen<string>("core_call_ended", (event) => {
    const { callTokens } = useStore.getState();
    if (!callTokens || callTokens.callId !== event.payload) return;
    endCall(event.payload);
  });
}

export function listenForCoreConnectionFailure(failed: (callId: string, reason: string) => void) {
  return listen<{ call_id: string; reason: string }>("core_room_connection_failed", (event) => {
    const { callTokens } = useStore.getState();
    if (!callTokens || callTokens.callId !== event.payload?.call_id) return;
    failed(event.payload.call_id, event.payload.reason);
  });
}
