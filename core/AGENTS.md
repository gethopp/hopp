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
- Variable names should be descriptive and not just letters, e.g. not use ss for screen_sharing.

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
- Always add code to your plan. The implementor agent shouldn't have to read the code in order to implement it. Make this explicit.

## New folders
- When creating new folders don't add a `mod.rs` file.

## Avoid at all costs
- Running cargo fmt
- Running cargo clippy

## Workflow Orchestration

### 1. Plan Node Default
- Enter plan mode for ANY non-trivial task (3+ steps or architectural decisions).
- If something goes sideways, **STOP** and re-plan immediately – don't keep pushing.
- Use plan mode for verification steps, not just building.
- Write detailed specs upfront to reduce ambiguity.

### 2. Subagent Strategy
- Use subagents liberally to keep main context window clean.
- Offload research, exploration, and parallel analysis to subagents.
- For complex problems, throw more compute at it via subagents.
- One task per subagent for focused execution.

### 3. Self-Improvement Loop
- Write rules for yourself that prevent the same mistake.
- Ruthlessly iterate on these lessons until mistake rate drops.
- Review lessons at session start for relevant project.

### 4. Verification Before Done
- Never mark a task complete without proving it works.
- Diff behavior between main and your changes when relevant.
- Ask yourself: "Would a staff engineer approve this?"
- demonstrate correctness.

### 5. Demand Elegance (Balanced)
- For non-trivial changes: pause and ask "is there a more elegant way?"
- If a fix feels hacky: "Knowing everything I know now, implement the elegant solution."
- Skip this for simple, obvious fixes – don't over-engineer.
- Challenge your own work before presenting it.

## Task Management

1. **Plan First**: Write plan to `tasks/todo.md` with checkable items.
2. **Verify Plan**: Check in before starting implementation.
3. **Track Progress**: Mark items complete as you go.

## Core Principles

- **Simplicity First**: Make every change as simple as possible. Impact minimal code.
- **No Laziness**: Find root causes. No temporary fixes. Senior developer standards.
- **Minimal Impact**: Changes should only touch what's necessary. Avoid introducing bugs.
