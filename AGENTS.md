# Agent Development Guide for Hopp

## Project Overview

Hopp is an open-source pair programming app with screen sharing, remote control, and multi-user rooms. Built with Tauri (desktop), Go (backend), and Rust (core engine).

## Directory Structure

- `backend/` — Go API server (Echo, PostgreSQL, Redis)
- `core/` — Rust screen capture/remote control engine
- `tauri/` — Tauri desktop app (React + TypeScript frontend)
- `web-app/` — React web application
- `docs/` — Astro documentation site

## Commands

All commands use [Taskfile](https://taskfile.dev). Run `task --list` in any directory to see available tasks.

Avoid running the following commands as an agent, as this is preferred to run from a user in their terminal and navigate in the Desktop app.

**Backend (Go):**
```bash
cd backend
task run          # Run with hot reload (Air)
task test         # Run tests
```

**Core (Rust):**
```bash
cd core
cargo build
cargo test
cargo fmt         # Format code
```

**Tauri App:**
```bash
cd tauri
task dev          # Dev mode with hot reload
task build        # Production build
```

**Web App:**
```bash
cd web-app
yarn dev
yarn build
```

## Code Style

- **JS/TS:** Prettier (120 cols). Runs via pre-commit.
- **Rust:** `cargo fmt` per crate (`core/`, `tauri/src-tauri/`).
- **Go:** `gofmt` + `golangci-lint` (config: `.golangcli.yml`).

Pre-commit hooks enforce all formatting automatically.

## Testing

- **Go:** Integration tests in `backend/test/integration/`
- **Rust:** Unit tests + visual integration tests in `core/tests/`
- **Frontend:** Linting + typechecking (no unit test runner configured)

## Key Conventions

- Use `@` alias for imports in TS/React code (maps to `src/`)
- API contracts defined in `backend/api-files/openapi.yaml`
- Desktop app launches `hopp_core` binary and communicates over socket
- Cross-platform: macOS, Windows, Linux all supported

<!-- OPENSPEC:START -->
# OpenSpec Instructions

These instructions are for AI assistants working in this project.

Always open `@/openspec/AGENTS.md` when the request:
- Mentions planning or proposals (words like proposal, spec, change, plan)
- Introduces new capabilities, breaking changes, architecture shifts, or big performance/security work
- Sounds ambiguous and you need the authoritative spec before coding

Use `@/openspec/AGENTS.md` to learn:
- How to create and apply change proposals
- Spec format and conventions
- Project structure and guidelines

Keep this managed block so 'openspec update' can refresh the instructions.

<!-- OPENSPEC:END -->
