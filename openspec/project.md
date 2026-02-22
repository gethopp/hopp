# Project Context

## Purpose

Hopp is an open-source **pair programming** app with:

- High-quality, low-latency screen sharing (WebRTC)
- Multi-user rooms ("mob programming")
- Remote control (mouse/keyboard) and remote cursors
- Team/workspace concepts (users, teams, invitations)

The desktop app is built with **Tauri** and a separate Rust "core process" (`hopp_core`). WebRTC infrastructure is powered by **LiveKit**.

## Tech Stack

This repo is a monorepo (Yarn workspaces) with multiple runtimes.

### Monorepo / Tooling

- **Yarn 4** workspaces (`packageManager: yarn@4.9.2`) with `nodeLinker: node-modules`
- **Node.js**: `v20` (`.nvmrc`)
- **Taskfile** (`task`) is the primary dev/build entrypoint across workspaces
- **Formatting / linting** enforced via pre-commit:
  - JS/TS: **Prettier** (`printWidth: 120`) via `pretty-quick --staged`
  - Rust: `cargo fmt` (per-crate)
  - Go: `golangci-lint` + `gofmt` (via `.golangcli.yml`)

### Backend (`/backend`)

- **Go 1.25** (`go.mod`)
- HTTP framework: **Echo**
- Datastores:
  - **PostgreSQL** (primary)
  - **Redis** (pub/sub + realtime event fanout; used for call notifications, etc.)
- API spec: **OpenAPI** (`backend/api-files/openapi.yaml`)
- ORM / persistence: **GORM** (Postgres driver; SQLite driver also present for testing)
- Auth: JWT + social auth (Google/GitHub via Goth)
- Integrations observed in deps:
  - Payments: **Stripe**
  - Observability: **Sentry**, **Prometheus**
  - Comms: **Slack**, **Resend** (email), Telegram notifications
  - Realtime: **Gorilla WebSocket**, **LiveKit server SDK**

### Desktop App (`/tauri`)

- **Tauri 2** (Rust backend + Vite frontend)
- Rust crate: `tauri/src-tauri` (Tauri plugins: updater, autostart, global shortcut, deep-link, etc.)
- Frontend: **React + TypeScript + Tailwind CSS** (Vite)
- Observability: Sentry (optional via `SENTRY_AUTH_TOKEN`)
- Updater: GitHub Releases (`latest.json` endpoint in `tauri.conf.json`)

### Core Process (`/core`)

- **Rust** "core engine" (`hopp_core`) handling:
  - Screen capture + streaming integration with LiveKit
  - Remote cursor rendering overlay
  - Remote input control (mouse/keyboard)
  - **Camera window** — standalone GPU-rendered window (winit + iced + wgpu) showing a participant video grid with mic/video/screenshare/end-call controls. Owned by `Application.camera_window`.
  - **Screensharing window** — standalone GPU-rendered window (winit + iced + wgpu) showing the remote screen share stream with a segmented control (Remote Control / Draw / Click Animation) and input event forwarding to the sharer via `RoomService`. Owned by `Application.screensharing_window`.
- Both windows are **native OS windows**, not Tauri webviews. They use `iced` for the widget tree, `wgpu` for GPU rendering, and `winit` for the event loop. The core `Application` struct implements `winit::ApplicationHandler` and routes `WindowEvent`s to the correct window by `WindowId`.
- The Tauri app launches the core process as a sidecar and communicates over a local socket (see IPC section below).

### Web App (`/web-app`)

- **React + TypeScript** (Vite)
- Routing: React Router (`react-router-dom`)
- Data fetching: TanStack React Query (and generated OpenAPI types)
- UI: Radix + Headless UI; styling via Tailwind CSS
- Build output is bundled as a single-file asset and injected into the backend's `backend/web/*.html`

### Documentation (`/docs`)

- **Astro + Starlight** (Tailwind)

## Tauri ↔ Core IPC Architecture

The desktop app has a three-layer communication model:

```
Tauri UI (React)  ←→  Tauri Backend (Rust)  ←→  Core Process (Rust)
   (webview)            (tauri commands)          (hopp_core sidecar)
```

### Tauri UI → Tauri Backend (Commands)

The React frontend calls Tauri commands via `invoke()`. All commands and their argument/return types are defined in `tauri/src/core_payloads.ts` inside the `CommandMap` interface. A typed wrapper `typedInvoke<K>()` provides compile-time type safety:

```typescript
// tauri/src/core_payloads.ts
export function typedInvoke<K extends keyof CommandMap>(
  cmd: K,
  ...args: InvokeArgs<K>
): Promise<CommandMap[K]["return"]>;
```

When adding a new Tauri command, add its entry to `CommandMap` in `core_payloads.ts` and the corresponding `#[tauri::command]` function in `tauri/src-tauri/src/main.rs`.

### Tauri Backend ↔ Core Process (Socket IPC)

Communication uses `socket_lib` (`core/socket_lib/src/lib.rs`):

- **Transport**: Unix domain socket (macOS/Linux) or localhost TCP (Windows).
- **Protocol**: Length-prefixed JSON. Each message is serialized via `serde_json`, prefixed with a `usize` length in little-endian bytes.
- **Message enum**: `socket_lib::Message` — all variants are defined in `core/socket_lib/src/lib.rs`. This is the single source of truth for the IPC contract.
- **Routing**: The `EventSocket` splits incoming messages into two channels:
  - `events` — fire-and-forget messages (e.g. `Ping`, `CallStart`, `OpenCamera`)
  - `responses` — request/response pairs (e.g. `GetAvailableContent` → `AvailableContent`, `StartScreenShare` → `StartScreenShareResult`)
  - `Message::is_response()` determines which channel receives a message.
- **Pattern**: Tauri backend sends a `Message` via `SocketSender`, then either ignores the result (fire-and-forget) or blocks on `event_socket.responses.recv_timeout()` for request/response commands.

### Core Process → Tauri UI (Event Forwarding)

Some events originate in the core process (e.g. participant state changes, role changes, call ended from camera/screensharing window). These flow:

1. Core sends a `Message` variant (e.g. `ParticipantsSnapshot`, `RoleChange`, `CallEnded`) back over the socket.
2. The Tauri backend's `forward_core_events()` thread receives it from the `events` channel.
3. It calls `app.emit("core_<event_name>", payload)` to broadcast to all Tauri webview windows.
4. The React frontend listens via `listen("core_<event_name>", callback)` from `@tauri-apps/api/event`.

Some of the current forwarded events for example: `core_participants_snapshot`, `core_role_change`, `core_camera_failed`, `core_call_ended`.

### Adding a New Message

1. Add the variant to `enum Message` in `core/socket_lib/src/lib.rs` (and any associated struct).
2. If the message is a response, add it to `Message::is_response()`.
3. Handle it in `core/src/lib.rs` (`user_event` or `handle_message`).
4. If it flows to Tauri UI: handle it in `forward_core_events()` in `tauri/src-tauri/src/main.rs` and `listen()` in the frontend.
5. Mirror any new structs in `tauri/src/core_payloads.ts` for frontend type safety.

## Project Conventions

### Code Style

- **Prettier** is the source of truth for JS/TS formatting (`.prettierrc`, 120 cols).
- **Rust** must be formatted with `cargo fmt` (pre-commit hook runs per crate: `core/`, `tauri/src-tauri/`).
- **Go** must be formatted with `gofmt` and pass `golangci-lint` (enabled linters are `govet`, `ineffassign`, `unused`, `staticcheck`).
- **Imports / aliases**:
  - JS/TS code uses `@` as an alias to `src/` (Vite config in both `tauri/` and `web-app/`).
- **Frontend security (XSS/URLs)**:
  - Prefer normal React bindings (`{value}`) over HTML injection.
  - Avoid `dangerouslySetInnerHTML` unless sanitized (e.g. DOMPurify).
  - Prefer `new URL()` / `URLSearchParams` for building URLs and query params over string concatenation.

### Architecture Patterns

- **Three-layer product shape**:
  - `core/`: low-level capture/remote-control engine (Rust)
  - `tauri/`: desktop shell + UI (Tauri + React)
  - `backend/`: API + auth + billing + integrations (Go)
  - `web-app/`: browser app/UI that can also be served/embedded via the backend
- **API contracts**:
  - The OpenAPI spec (`backend/api-files/openapi.yaml`) is treated as the contract; type-safe clients are generated from it.
  - The socket IPC contract is `socket_lib::Message` enum; the TypeScript mirror is `CommandMap` + struct interfaces in `tauri/src/core_payloads.ts`.
- **Local dev ergonomics**:
  - `task` is used to coordinate multi-service dev (backend + livekit + webapp builds).
  - Web app assets are built and injected into `backend/web/` for backend-driven pages.

### Testing Strategy

- **Backend (Go)**:
  - Integration tests live under `backend/test/integration/`.
- **Core (Rust)**:
  - Rust unit tests are limited; there are **visual integration tests** under `core/tests/` (see `core/tests/README.md`).
- **Frontend (web-app/tauri)**:
  - Linting + typechecking + manual QA are currently the primary guardrails (no dedicated unit test runner is configured in package manifests).

### Git Workflow

- Use feature branches and open PRs against the default branch.
- Expect pre-commit hooks to run on commit (Prettier, `cargo fmt`, `golangci-lint`).
- Keep commits focused and descriptive; avoid mixing formatting-only changes with behavioral changes unless necessary.

## Domain Context

- **Rooms**: sessions where multiple participants can join for pairing/mobbing.
- **Sharer vs controller**:
  - The **sharer** is streaming their screen and can allow remote control.
  - **Controllers** view the stream and can send input events (mouse/keyboard) over WebRTC data channels.
- **LiveKit**:
  - Used for media (video) and data channels (control events, cursor positions, etc.).
- **Desktop UX**:
  - Tauri provides tray/main-window surfaces; `hopp_core` owns the camera and screensharing windows (native GPU-rendered) and interacts with OS APIs.

## Important Constraints

- **Cross-platform desktop**: macOS/Windows/Linux constraints and platform APIs matter (capture, overlays, input simulation).
- **Local HTTPS requirement**: backend local dev uses mkcert-generated certs for HTTPS/websocket flows (WebKit requirements).
- **Fixed dev port expectations**: Tauri dev expects a fixed Vite dev port (default `1420`).
- **Bundling constraint**: the desktop bundle includes the `hopp_core` binary as an external resource.
- **License**: AGPL-3.0-only (see root `package.json` / repo license).

## External Dependencies

- **LiveKit** (self-hosted or cloud) for WebRTC rooms/streaming
- **PostgreSQL** database
- **Redis** for pub/sub and realtime events
- **Sentry** for error reporting (Go + desktop UI)
- **PostHog** for product analytics (web/desktop UI)
- **Stripe** for billing/subscriptions
- **Slack** integration
- **Resend** for transactional email
- **GitHub Releases** for Tauri updater artifacts (`latest.json`)
