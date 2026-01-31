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
- Core subsystems: `capture/`, `graphics/`, `input/`, `room_service/`.
- Logging uses `env_logger`; set `RUST_LOG=hopp_core=info` (use `debug` only when needed).

## Related Docs

- `README.md` for architecture and diagrams.
- `tests/README.md` for test setup and commands (manual only).
