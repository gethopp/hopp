# Agent Development Guide for Hopp

## Project Overview

Hopp is an open-source pair programming app with screen sharing, remote control, and multi-user rooms. Built with Tauri (desktop), Go (backend), and Rust (core engine).

## Directory Structure

- `backend/` — Go API server (Echo, PostgreSQL, Redis)
- `core/` — Rust screen capture/remote control engine
- `tauri/` — Tauri desktop app (React + TypeScript frontend)
- `web-app/` — React web application
- `docs/` — Astro documentation site

## Tech Stack

- **Monorepo**: Yarn 4 workspaces, Node.js v20, Taskfile as primary entrypoint
- **Backend**: Go 1.25, Echo, PostgreSQL, Redis, GORM, JWT auth, Stripe, Sentry, LiveKit server SDK
- **Desktop**: Tauri 2 (Rust + React/TypeScript/Tailwind, Vite)
- **Core**: Rust — screen capture, remote input, LiveKit streaming, camera window and screensharing window (winit + iced + wgpu, native OS windows, not Tauri webviews)
- **Web App**: React + TypeScript, Vite, TanStack Query, Radix + Headless UI, Tailwind

## Commands

All commands use [Taskfile](https://taskfile.dev). Run `task --list` in any directory to see available tasks.

Do not run these as an agent — they require user interaction in a terminal/desktop.

**Backend:** `cd backend && task run` / `task test`
**Core:** `cd core && cargo build` / `cargo test` / `cargo fmt`
**Tauri:** `cd tauri && task dev` / `task build`
**Web App:** `cd web-app && yarn dev` / `yarn build`

## IPC Architecture

```
Tauri UI (React)  ←→  Tauri Backend (Rust)  ←→  Core Process (Rust)
   (webview)            (tauri commands)          (hopp_core sidecar)
```

- **UI → Tauri**: `invoke()` calls typed via `CommandMap` in `tauri/src/core_payloads.ts`
- **Tauri ↔ Core**: `socket_lib` — Unix socket (macOS/Linux) or TCP (Windows), length-prefixed JSON. All message variants in `core/socket_lib/src/lib.rs` (`enum Message`)
- **Core → UI**: Core sends a `Message`, `forward_core_events()` in `tauri/src-tauri/src/main.rs` emits `app.emit("core_<event>", payload)`, frontend listens via `listen("core_<event>", cb)`

### Adding a new IPC message

1. Add variant to `enum Message` in `core/socket_lib/src/lib.rs`
2. If it's a response, add to `Message::is_response()`
3. Handle in `core/src/lib.rs` (`handle_message` or `user_event`)
4. If forwarded to UI: handle in `forward_core_events()` and `listen()` in frontend
5. Mirror new structs in `tauri/src/core_payloads.ts`

## Code Style

- **JS/TS:** Prettier (120 cols). Pre-commit runs automatically.
- **Rust:** `cargo fmt` per crate (`core/`, `tauri/src-tauri/`).
- **Go:** `gofmt` + `golangci-lint` (config: `.golangcli.yml`; linters: `govet`, `ineffassign`, `unused`, `staticcheck`).
- **TS imports:** Use `@` alias → `src/` (configured in Vite for both `tauri/` and `web-app/`)
- **Frontend security:** Prefer normal React bindings; avoid `dangerouslySetInnerHTML`; use `new URL()` / `URLSearchParams` over string concatenation for URLs.

## Testing

- **Go:** Integration tests in `backend/test/integration/`
- **Rust:** Unit tests + visual integration tests in `core/tests/`
- **Frontend:** Linting + typechecking (no unit test runner configured)

## Key Conventions

- API contract: `backend/api-files/openapi.yaml` (OpenAPI); type-safe clients generated from it
- IPC contract: `socket_lib::Message` enum + TypeScript mirror in `tauri/src/core_payloads.ts`
- Cross-platform desktop: macOS/Windows/Linux — platform APIs and capture/overlay/input constraints matter
- Local dev uses mkcert HTTPS certs (WebKit requirement); Tauri dev expects Vite on port `1420`
- `hopp_core` binary bundled as external resource in the desktop bundle
