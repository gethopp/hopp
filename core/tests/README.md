# Hopp Core Tests

This directory contains integration tests for the Hopp Core application, focusing on testing remote control functionality through LiveKit.

## Overview

The test suite provides automated testing for:

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