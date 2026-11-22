/**
 * Tests for the Slack join-session deep link handler.
 *
 * The app has no frontend test runner, so this runs on `node --test` and
 * transpiles the real `deepLinkUtils.ts` in memory, injecting fake modules for
 * its imports. That keeps the assertions against the actual handler code
 * instead of a copy of it.
 *
 * Run with:
 *   cd tauri && node --test src/lib/deepLinkUtils.test.mjs
 */
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import ts from "typescript";

const sourcePath = path.join(path.dirname(fileURLToPath(import.meta.url)), "deepLinkUtils.ts");

const SESSION_ID = "session-123";
const TOKENS = { audioToken: "audio-token", videoToken: "video-token", participant: "participant-1" };

/**
 * Transpiles deepLinkUtils.ts and loads it with fake modules, returning the
 * module exports plus the recorded calls made against those fakes.
 */
function loadDeepLinkUtils({ authToken = "auth-token", callTokens = null, fetchImpl, callStartedImpl } = {}) {
  const calls = [];
  const record = (name, ...args) => calls.push({ name, args });

  const state = {
    authToken,
    callTokens,
    user: { id: "user-1" },
    setCallTokens: (tokens) => {
      state.callTokens = tokens;
      record("setCallTokens", tokens);
    },
    setTab: (tab) => record("setTab", tab),
  };

  const useStore = { getState: () => state };

  const tauriUtils = {
    showWindow: async (label) => record("showWindow", label),
    getCallStartPreferences: async () => ({ startMic: true, startCamera: false }),
    callStarted: async (audioToken, videoToken) => {
      record("callStarted", audioToken, videoToken);
      if (callStartedImpl) await callStartedImpl();
    },
    endCallCleanup: () => record("endCallCleanup"),
    stopSharing: () => record("stopSharing"),
  };

  const toast = (message) => record("toast", message);
  toast.error = (message) => record("toast.error", message);
  toast.loading = (message, options) => record("toast.loading", message, options);
  toast.dismiss = (id) => record("toast.dismiss", id);
  toast.success = (message) => record("toast.success", message);

  const mocks = {
    "react-hot-toast": { __esModule: true, default: toast },
    "@/store/store": { __esModule: true, default: useStore, ParticipantRole: { NONE: "none", SHARER: "sharer" } },
    "@/windows/window-utils": { __esModule: true, tauriUtils },
    "@/constants": { __esModule: true, Constants: { backendUrl: "https://example.test" } },
    "@/services/socket": {
      __esModule: true,
      socketService: { send: (message) => record("socket.send", message) },
    },
    "./authUtils": { __esModule: true, validateAndSetAuthToken: async () => record("validateAndSetAuthToken") },
  };

  const fetchMock = async (url, options) => {
    record("fetch", url, options?.method ?? "GET");
    return fetchImpl ? fetchImpl(url, options) : jsonResponse(TOKENS);
  };

  const { outputText } = ts.transpileModule(readFileSync(sourcePath, "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  });

  const module = { exports: {} };
  const requireShim = (specifier) => {
    if (!(specifier in mocks)) throw new Error(`Unexpected import in deepLinkUtils.ts: ${specifier}`);
    return mocks[specifier];
  };

  // eslint-disable-next-line no-new-func
  new Function("module", "exports", "require", "fetch", outputText)(module, module.exports, requireShim, fetchMock);

  return { exports: module.exports, calls, state, names: () => calls.map((call) => call.name) };
}

const jsonResponse = (body, status = 200) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
});

const find = (calls, name) => calls.find((call) => call.name === name);

test("starts the call in core with the fetched tokens", async () => {
  const { exports, calls } = loadDeepLinkUtils();

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, true);
  assert.deepEqual(find(calls, "callStarted").args, [TOKENS.audioToken, TOKENS.videoToken]);
  assert.equal(find(calls, "setCallTokens").args[0].isInitialisingCall, true);
  assert.equal(find(calls, "setCallTokens").args[0].room.id, SESSION_ID);
});

test("does not set a tab on a successful join, leaving call navigation to the app", async () => {
  const { exports, calls } = loadDeepLinkUtils();

  await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(find(calls, "setTab"), undefined);
});

test("clears call state and cleans up when core fails to start the call", async () => {
  const { exports, calls, state } = loadDeepLinkUtils({
    callStartedImpl: async () => {
      throw new Error("core refused to start the call");
    },
  });

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, false);
  // Call state must be rolled back, otherwise the UI shows a call that is not running.
  assert.equal(state.callTokens, null);
  assert.ok(find(calls, "endCallCleanup"), "core cleanup should run after a failed start");
  assert.deepEqual(find(calls, "toast.error").args, ["Failed to start call"]);

  // Same rollback as the shared join path: tell the server the call ended,
  // using the participant from the tokens we were given.
  assert.deepEqual(find(calls, "socket.send").args[0], {
    type: "call_end",
    payload: { participant_id: TOKENS.participant },
  });
});

test("surfaces the existing call instead of rejoining the same session", async () => {
  const { exports, calls } = loadDeepLinkUtils({
    callTokens: { room: { id: SESSION_ID }, participant: "p" },
  });

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, true);
  assert.deepEqual(find(calls, "setTab").args, ["call"]);
  assert.equal(find(calls, "fetch"), undefined, "should not refetch tokens for the call it is already in");
  assert.equal(find(calls, "callStarted"), undefined);
});

test("refuses to join while a different call is active, leaving that call untouched", async () => {
  const activeCall = { room: { id: "other-room" }, participant: "p-old", role: "none" };
  const { exports, calls, state } = loadDeepLinkUtils({ callTokens: activeCall });

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, false);
  assert.deepEqual(find(calls, "toast.error").args, ["Leave your current call first"]);
  // The active call must survive untouched: no teardown, no token fetch, no new call.
  assert.equal(state.callTokens, activeCall);
  assert.equal(find(calls, "fetch"), undefined);
  assert.equal(find(calls, "callStarted"), undefined);
  assert.equal(find(calls, "endCallCleanup"), undefined);
  assert.equal(find(calls, "socket.send"), undefined);
});

test("does not overwrite a call that starts while the token fetch is pending", async () => {
  let state;
  const { exports, calls, ...loaded } = loadDeepLinkUtils({
    fetchImpl: async () => {
      // A call begins after the deep link was accepted but before tokens arrive.
      state.callTokens = { room: { id: "raced-room" }, participant: "p-race" };
      return jsonResponse(TOKENS);
    },
  });
  state = loaded.state;

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, false);
  assert.equal(state.callTokens.room.id, "raced-room", "the racing call must survive");
  assert.equal(find(calls, "callStarted"), undefined);
  assert.deepEqual(find(calls, "toast.error").args, ["Leave your current call first"]);
});

test("does not start a call when the session is gone", async () => {
  const { exports, calls } = loadDeepLinkUtils({ fetchImpl: async () => jsonResponse({}, 404) });

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, false);
  assert.equal(find(calls, "callStarted"), undefined);
  assert.deepEqual(find(calls, "toast.error").args, ["Session has ended or doesn't exist"]);
});

test("asks the user to log in when there is no auth token", async () => {
  const { exports, calls } = loadDeepLinkUtils({ authToken: null });

  const joined = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(joined, false);
  assert.deepEqual(find(calls, "setTab").args, ["login"]);
  assert.equal(find(calls, "fetch"), undefined);
});

test("routes a join-session deep link URL to the join handler", async () => {
  const { exports, calls } = loadDeepLinkUtils();

  const handled = await exports.processDeepLinkUrl(`hopp:///join-session?sessionId=${SESSION_ID}`);

  assert.equal(handled, true);
  assert.deepEqual(find(calls, "callStarted").args, [TOKENS.audioToken, TOKENS.videoToken]);
});

test("ignores deep links that are not hopp:// URLs", async () => {
  const { exports, calls } = loadDeepLinkUtils();

  const handled = await exports.processDeepLinkUrl(`https://example.test/join-session?sessionId=${SESSION_ID}`);

  assert.equal(handled, false);
  assert.equal(find(calls, "fetch"), undefined);
});

test("ignores a second deep link while a join is already in progress", async () => {
  let releaseTokens;
  const tokensPending = new Promise((resolve) => {
    releaseTokens = resolve;
  });
  const { exports, calls } = loadDeepLinkUtils({
    fetchImpl: async () => {
      await tokensPending;
      return jsonResponse(TOKENS);
    },
  });

  // Start one join and leave it waiting on the token fetch, then fire a second.
  const firstJoin = exports.handleJoinSessionDeepLink(SESSION_ID);
  const secondJoin = await exports.handleJoinSessionDeepLink(SESSION_ID);

  assert.equal(secondJoin, false, "the second deep link must not start a competing join");

  releaseTokens();
  assert.equal(await firstJoin, true, "the first join should still complete");
  assert.equal(
    calls.filter((call) => call.name === "callStarted").length,
    1,
    "core should be asked to start the call exactly once",
  );
});
