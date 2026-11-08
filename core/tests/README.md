# Hopp Core Tests

This directory contains integration tests for the Hopp Core application, focusing on testing remote control functionality through LiveKit.

## Automated local smoke tests

Run from `core/`:

```sh
task smoke -- --list
task smoke                                      # standard suite, ten cycles
task smoke -- screenshare-call screenshare-room viewer-lifecycle
task smoke -- camera microphone combined --require-devices
task smoke -- call-lifecycle call-races --cycles 30 --timeout-seconds 900
```

The existing Rust harness builds a `test-support` core and animated fixture,
starts local LiveKit when needed, and runs each scenario against a fresh core in
an isolated room. The scenario repeats actions within that same core to exercise
reuse and teardown. An independent parent deadline kills hung processes and
their fixture children. Results and separate core/scenario logs are saved under
`tests/out/smoke-<run>/`. No production backend, account, or second person is needed.
Keep its LiveKit SDK/lockfile on the same maintained commit as core; different
native WebRTC builds can negotiate different H.264 profiles.

Prerequisites: stable Rust, `livekit-server` (`brew install livekit` on macOS), a
logged-in desktop, an audio input/output device, and Microphone permission for
the launching terminal/app. Camera scenarios need Camera permission and a camera;
screen scenarios need Screen Recording. Remote input checks may also require
Accessibility. Grant permissions through macOS when prompted, then rerun. The
harness does not alter OS permissions or install virtual devices.

`LIVEKIT_URL`, `LIVEKIT_API_KEY`, and `LIVEKIT_API_SECRET` override the local
defaults (`ws://127.0.0.1:7880`, `devkey`, `secret`). An unavailable explicitly
configured remote server is an error. `--core-bin PATH` reuses a built
`test-support` core and its sibling `hopp_test_fixture`; rebuild after source changes.

| Scenario | Intended coverage |
| --- | --- |
| `call-lifecycle` | Call join/end/rejoin, presence and continued main-loop responsiveness |
| `call-races` | Early hang-up, stale hang-up/camera actions, recovery in the next call |
| `screenshare-call` | Normal pair-call screen publishing, decoded content, start/stop/restart and control changes |
| `screenshare-room` | Multi-user room screen publishing, participant changes and control handoff |
| `viewer-lifecycle` | Remote sharing, mute/unmute storm, viewer close/reopen, replacement sharer and fresh rendering |
| `camera` | Local camera delivery/restart, remote camera/window lifecycle, and camera-off during connection |
| `camera-viewer` | Synthetic remote camera decoding/presentation and window reuse without a local camera |
| `microphone` | Real captured PCM delivery, mute/unmute/capture restart, incoming signal receive/mix/output |
| `audio-signal` | Opt-in configured loopback: known audio signal reaches another participant, mute suppresses it, unmute restores it |
| `combined` | Screen, camera and microphone together; changing one preserves the others |
| `controls` | Native call controls, picker cancellation, drawing/settings and stats window lifecycle |
| `network-drop` | Explicit opt-in outage during a live call, followed by media recovery |

Pair calls and named rooms use the same core media protocol; these tests exercise
the two-person and multi-user media/control paths with production-style identities.
Backend room creation/join APIs and Tauri UI orchestration are separate coverage.

Native close/hide is checked separately from stopping publication: hiding the
camera/viewer window must not be mistaken for switching off its media. The
feature-gated status/actions exercise production handlers and report actual
decode/present/audio progress. Normal builds omit these hooks and counters.

Missing prerequisites produce `SKIP` with a reason. Use `--require-devices` for a
complete media run: missing required hardware/permissions then fails. An existing
device failing to start, missing decoded media, or a timeout is a failure.

A quiet microphone or covered camera is valid. PCM/frame delivery does not prove
physical sound/picture quality. Known audio arriving in core is checked through
its output path; physical speaker audibility/echo and real microphone signal
quality still require a short hardware check or explicitly configured loopback.
No loopback driver is installed automatically. Actual title-bar click/shortcut
delivery and visual layout remain desktop checks.

The camera-off-during-connection regression uses a loopback signaling gate with
`ws://` LiveKit. It processes camera-off before allowing connection to finish,
then checks capture stays off. The earlier camera checks verify actual capture
and decoded delivery so unavailable camera hardware cannot mask this regression.

Device overrides: `HOPP_TEST_MICROPHONE` and `HOPP_TEST_CAMERA` select explicit
device names. To validate a nonzero microphone signal automatically, configure
system-output-to-loopback routing yourself, then set
`HOPP_TEST_LOOPBACK_MICROPHONE` to that input and run `task smoke -- audio-signal
--require-devices`. This plays a quiet generated PCM fixture through `afplay`;
normal microphone selection and system audio routing are not changed. The
loopback scenario and `network-drop` are excluded from the default suite.

The low-level `StartAudioCapture` command selects a device while capture is
active. `StopAudioCapture` tears capture down; the restart check then starts a
new call. Normal in-call microphone off/on is tested through mute/unmute/toggle.
Keyboard/click checks target the owned fixture. Clipboard scenarios remain
manual so the suite does not overwrite the user's clipboard formats.
Control tests use the real pointer/keyboard; local sharer input can reclaim control.

### When to run

Run relevant tests while developing, after every commit, and on the final working
tree before handoff. Do not create commits just to trigger tests. There are no
harness build/run jobs in CI; local runs are the required workflow.

| Changed code | Local checks |
| --- | --- |
| IPC, shared call lifecycle, runner | Socket/core library tests, harness unit tests, full smoke suite |
| Screen capture/viewer/control | `screenshare-call screenshare-room viewer-lifecycle controls combined` |
| Camera/windows | `camera camera-viewer controls combined call-races` |
| Microphone/audio | `microphone combined call-lifecycle` |
| Native controls/picker/drawing | `controls viewer-lifecycle` plus relevant existing manual input scenarios |
| Tauri call/shortcut integration | Desktop Rust regressions plus corresponding media smoke scenarios |

Harness unit checks: `cargo test --manifest-path tests/Cargo.toml` from `core/`.
Core/library/build checks follow `core/AGENTS.md`. Record skips and platform gaps
with results. Automated live runs initially target macOS; do not infer Windows or
Linux coverage from a Mac run.

### Bad connections (opt-in)

```sh
sudo -v
task smoke -- screenshare-call microphone --net bad --require-devices
task smoke -- network-drop --require-devices
```

Profiles `good`, `medium`, `bad`, and `3g` add bandwidth limits, latency and loss.
`network-drop` cuts traffic mid-call; `--net drop` is deliberately unavailable.
The macOS `netem.sh` uses dedicated per-run PF anchors/pipes and releases its own
PF enable reference on cleanup, preserving other rules/sessions. Sudo credentials
must already be available; shaping failure is an error. If cleanup fails, the
runner prints the exact `off --session` recovery command. Never run these against
a production server shared with unrelated calls.

Offline network-rule checks: `./tests/netem.sh --self-test`.

## Existing manual scenarios

The existing commands drive these manual scenarios:

- **Cursor functionality** - Testing remote cursor movement, clicks, scrolling, and multi-participant scenarios
- **Keyboard functionality** - Testing remote keyboard input and character transmission
- **Clipboard functionality** - Testing clipboard operations including copy, cut, and paste with single and multiple payloads
- **Screenshare functionality** - Testing screen sharing capabilities via socket communication

## Prerequisites

- Rust (latest stable version)
- LiveKit server instance with API credentials
- Core process running (use `task dev` from the core directory)

## Setup

### 1. Install Dependencies

```bash
cargo build
```

### 2. Environment Variables

Set the following environment variables:

- `LIVEKIT_URL`: The WebSocket URL of your LiveKit server
- `LIVEKIT_API_KEY`: Your LiveKit API key
- `LIVEKIT_API_SECRET`: Your LiveKit API secret
- `CONTENT_ID`: Source ID for the display to be shared (screen capture source identifier)

## Usage

Run tests using cargo:

```bash
cargo run -- <command> [options]
```

### Available Commands

#### Cursor Tests

Test various cursor functionalities:

```bash
# Basic cursor tests
cargo run -- cursor complete          # Run complete cursor test for single cursor
cargo run -- cursor click             # Test cursor clicking
cargo run -- cursor move              # Test cursor movement
cargo run -- cursor scroll            # Test cursor scrolling

# Multi-participant tests
cargo run -- cursor multiple-participants    # Test multiple participants
cargo run -- cursor cursor-control          # Test multiple cursors with control handoff
cargo run -- cursor staggered-joining       # Test staggered participant joining
cargo run -- cursor same-first-name-participants  # Test participants with same first names

# Advanced cursor behavior tests
cargo run -- cursor hide-on-inactivity      # Test cursor hiding after inactivity
cargo run -- cursor concurrent-scrolling    # Test concurrent scrolling scenarios
```

#### Keyboard Tests

Test keyboard input functionality:

```bash
# Test keyboard character input (lowercase, uppercase, numbers, symbols)
cargo run -- keyboard
```

#### Clipboard Tests

Test clipboard functionality:

```bash
# Test paste with single payload
cargo run -- clipboard paste-single

# Test paste with multiple payloads
cargo run -- clipboard paste-multiple

# Test add to clipboard (copy)
cargo run -- clipboard add-copy

# Test add to clipboard (cut)
cargo run -- clipboard add-cut
```

**Note:** The `add-copy` and `add-cut` tests require user interaction. You will be prompted to select text in any application before the test executes the copy/cut operation.

#### Screenshare Tests

Test screen sharing functionality:

```bash
# Test screenshare capabilities via socket communication
cargo run -- screenshare
```

### Help

Get help for available commands:

```bash
# General help
cargo run -- --help

# Help for specific commands
cargo run -- cursor --help
```

## License

This project follows the same license as the parent Hopp Core project.
