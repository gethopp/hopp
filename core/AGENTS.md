# Core Agent Guide

## Scope

This directory contains the Rust screen capture and remote control engine (`hopp_core`) plus supporting crates (`socket_lib`, `sentry_utils`).

## Key Commands

- See `Taskfile.yml` in this directory for the full, up-to-date command list.

## Building
- Verify your changes by running `task build_dev`

## Formatting & Linting

- `cargo fmt` is required (pre-commit formats staged Rust files).
- CI runs `cargo fmt --all -- --check` and `cargo clippy -D warnings`.
- Rust edition: 2021.

## Testing

- Do not run core tests as an agent.
- For validation, only use build commands (see `Taskfile.yml`).
- Full testing details live in `tests/README.md`.

## Conventions & Structure

- Platform-specific modules live in `src/**/{linux,macos,windows}.rs`.
- Platform modules use `#[cfg_attr(target_os = "macos", path = "macos.rs")] mod platform;` to conditionally select the platform file at compile time.
- Core subsystems: `capture/`, `graphics/`, `input/`, `room_service/`.
- Logging uses `env_logger`; set `RUST_LOG=hopp_core=info` (use `debug` only when needed).

## Architecture Overview

- Screen-sharing and remote-control engine: one user **shares** their screen, others **control** it remotely.
- Launched as a child process by a Tauri app; communicates with it via IPC socket (`socket_lib`).
- Main event loop is driven by `winit`; rendering uses `wgpu`.
- `RoomService` runs an async Tokio runtime managing a LiveKit/WebRTC room for media and data channels.
- Two roles: **Sharer** (captures screen, forwards remote input) and **Controller** (renders remote frames, sends input).
- CLI args: `--textures-path`, `--sentry-dsn`, `--socket-path`.

## Supporting Crates

- `socket_lib` — IPC layer between the Tauri app and the core process (Unix sockets on macOS/Linux, TCP on Windows). Defines all message types exchanged between the two processes.
- `sentry_utils` — Error reporting and telemetry via Sentry.

## Key Types

- `Application` — central state struct, owns all subsystems.
- `RenderEventLoop` — wraps the `winit` event loop and drives rendering.
- `UserEvent` — enum of all events flowing through the system (socket messages, room events, UI actions).
- `RemoteControl` — active session state holding graphics and input controllers.
- `RoomService` — manages the LiveKit/WebRTC room on an async Tokio runtime.

## Related Docs

- `README.md` for architecture and diagrams.
- `tests/README.md` for test setup and commands (manual only).

## Plan Mode

- Make the plan extremely concise. Sacrifice grammar for the sake of concision.
- At the end of each plan, give me a list of unresolved questions to answer, if any.

## New folders
- When creating new folders don't add a `mod.rs` file.
