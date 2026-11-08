// Run from the repository root: node --test tauri/tests/core-call-events.test.mjs
import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { build } from "esbuild";

test("a delayed call-ended event from A must not end the newer call B", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "hopp-call-events-"));
  let unlisten;
  let unlistenFailure;
  let unlistenProcessFailure;
  t.after(async () => {
    await unlisten?.();
    await unlistenFailure?.();
    await unlistenProcessFailure?.();
    clearMocks();
    delete globalThis.window;
    await rm(directory, { recursive: true, force: true });
  });
  globalThis.window = { crypto: webcrypto };
  let handleCommand = () => undefined;
  mockIPC((command, args) => handleCommand(command, args), { shouldMockEvents: true });
  mockWindows("main");

  const root = fileURLToPath(new URL("../", import.meta.url));
  const bundle = join(directory, "call-events.cjs");
  await build({
    stdin: {
      contents: `
        export { listenForCoreCallEnd, listenForCoreConnectionFailure, listenForCoreProcessFailure } from "./src/lib/core-call-events";
        export { default as useStore } from "./src/store/store";
        export { tauriUtils } from "./src/windows/window-utils";
      `,
      resolveDir: root,
      loader: "ts",
    },
    tsconfig: join(root, "tsconfig.json"),
    bundle: true,
    platform: "node",
    format: "cjs",
    outfile: bundle,
  });
  const { listenForCoreCallEnd, listenForCoreConnectionFailure, listenForCoreProcessFailure, useStore, tauriUtils } =
    await import(pathToFileURL(bundle));
  unlisten = await listenForCoreCallEnd(() => useStore.getState().setCallTokens(null));

  const call = (name) => ({
    callId: name,
    audioToken: `audio-${name}`,
    videoToken: `video-${name}`,
    cameraToken: `camera-${name}`,
    participant: name,
    timeStarted: new Date(),
    hasAudioEnabled: true,
    hasCameraEnabled: false,
    role: "none",
    isRemoteControlEnabled: false,
    participants: [],
    micLevel: 0,
  });
  useStore.getState().setCallTokens(call("A"));
  await emit("core_call_ended", "A");
  assert.equal(useStore.getState().callTokens, null, "the current call's event must still end its call");
  // A duplicate acknowledgement for A arrives after the UI has joined B.
  const deliverCallAEnded = () => emit("core_call_ended", "A");
  useStore.getState().setCallTokens(call("B"));
  await deliverCallAEnded();

  assert.equal(useStore.getState().callTokens?.participant, "B", "A's late event must leave B active");
  await emit("core_call_ended", null);
  assert.equal(useStore.getState().callTokens?.callId, "B", "an untagged notification cannot identify B");

  let failure;
  unlistenFailure = await listenForCoreConnectionFailure((callId, reason) => {
    failure = { callId, reason };
    useStore.getState().setCallTokens(null);
  });
  await emit("core_room_connection_failed", { call_id: "A", reason: "old failure" });
  assert.equal(useStore.getState().callTokens?.callId, "B", "A's late failure must leave B active");
  assert.equal(failure, undefined);
  await emit("core_room_connection_failed", { call_id: "B", reason: "current failure" });
  assert.equal(useStore.getState().callTokens, null);
  assert.deepEqual(failure, { callId: "B", reason: "current failure" });

  useStore.getState().setCallTokens(call("A"));
  let finishReset;
  handleCommand = (command, args) => {
    if (command === "reset_core_process") {
      assert.equal(args.callId, "A");
      return new Promise((resolve) => {
        finishReset = resolve;
      });
    }
    assert.ok(
      ["set_tray_notification", "toggle_call_sleep_prevention"].includes(command),
      `unexpected command ${command}`,
    );
  };
  const cleanupA = tauriUtils.endCallCleanup("A");
  useStore.getState().setCallTokens(call("B"));
  finishReset();
  await cleanupA;
  assert.equal(useStore.getState().callTokens?.callId, "B", "A's awaited cleanup must leave B active");

  useStore.getState().setCallTokens(call("A"));
  let finishStart;
  handleCommand = (command, args) => {
    if (command === "call_started") {
      assert.equal(args.callId, "A");
      return new Promise((resolve) => {
        finishStart = resolve;
      });
    }
  };
  const startA = tauriUtils.callStarted("A", "audio-A", "video-A");
  useStore.getState().setCallTokens(call("B"));
  finishStart();
  await assert.rejects(startA, /Call ended during startup/);
  assert.equal(useStore.getState().callTokens?.callId, "B");
  await assert.rejects(tauriUtils.callStarted("A", "audio-A", "video-A"), /Call ended before startup/);

  const endedCalls = [];
  const failures = [];
  let cleanup;
  handleCommand = (command, args) => {
    if (command === "reset_core_process") endedCalls.push(args.callId);
    if (command === "plugin:window|get_all_windows") return [];
    assert.ok(
      ["reset_core_process", "set_tray_notification", "toggle_call_sleep_prevention"].includes(command),
      `core failure must only clean up, unexpected command: ${command}`,
    );
  };
  unlistenProcessFailure = await listenForCoreProcessFailure(
    (callId) => {
      cleanup = tauriUtils.endCallCleanup(callId);
      useStore.getState().setCallTokens(null);
    },
    (reason) => {
      failures.push(reason);
    },
  );
  await emit("core_process_crashed", "Connection lost. Please restart Hopp.");
  await cleanup;
  assert.deepEqual(endedCalls, ["B"], "a failure ends the current call without a user action");
  assert.equal(useStore.getState().callTokens, null);
  await emit("core_process_crashed", "Core stopped.");
  assert.deepEqual(endedCalls, ["B"], "duplicate failures must not end the call twice");
  assert.deepEqual(failures, ["Connection lost. Please restart Hopp.", "Core stopped."]);
});
