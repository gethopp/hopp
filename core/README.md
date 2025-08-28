# Hopp Core

Hopp's remote desktop control and screen sharing engine. Handles screen capture, real-time streaming, multi-user input control, remote cursors
rendering.

## Quick Start

### Dependencies

We are using [Task](https://taskfile.dev/) as our build tool.

### Build and run

```bash
task build_dev
```

### Testing

Currently rust unit tests are missing (it's on our TODOs), but we have created a few visual integration
tests in the `tests` folder, for more details see the [README](tests/README.md).

A quick way to verify your changes would be to run `task build_dev` and then run a test from the `tests` folder.
For example:

```bash
task dev
# From a different terminal
cd tests/
cargo run -- cursor move
```

## Key Concepts

- **Sharer**: The user who is sharing their screen and allow remote control of their machine.
- **Controller**: A participant to the room who views the screen sharing stream and can
  control the sharer's machine.

## Overview

```mermaid
graph TD
    subgraph HoppCore [" HoppCore "]
        A[Capturer]
        B[CursorController]
        C[KeyboardController]
        D[RoomService]
        E[GraphicsContext]
    end

    %% External systems
    F[Tauri Application]
    G[Operating System]
    I[LiveKit Server]
    J[Socket]
    K[GPU/Display]

    %% Connections
    F <--> J
    J <--> HoppCore

    %% Input simulation
    B <--> G
    C <--> G

    %% Screen capture
    A --> G

    %% Streaming
    A --> I
    D <--> I

    %% Graphics
    E --> K

    %% Styling
    classDef core fill:#4fc3f7,stroke:#0277bd,stroke-width:2px,color:#000
    classDef external fill:#ffb74d,stroke:#f57c00,stroke-width:2px,color:#000
    classDef system fill:#81c784,stroke:#388e3c,stroke-width:2px,color:#000

    class A,B,C,D,E core
    class F,J external
    class G,I,K system
```

The `Tauri` app starts the core process and communicates with it via a socket.

`HoppCore` manages two primary subsystems: the `Capturer` object responsible for screen capture and screenshot
generation, and the `RoomService` which handles asynchronous `LiveKit` operations.

During a screen sharing session the following happens:

- `RoomService` connects to the `LiveKit` room and creates the video stream infrastructure.
- `Capturer` begins capturing the selected display and sends frames to `LiveKit`'s `NativeVideoSource` buffer for real-time streaming.
- An overlay window is created for rendering the virtual cursors.
- Remote control is handled via the cursor and keyboard controllers.
- Controller input events arrive as `LiveKit` `DataPackets` through the room service, get converted to `UserEvents` in the event loop, and are forwarded to the appropriate input controllers for processing.

### Remote Control Engine

```mermaid
graph TD
    subgraph RemoteControlEngine
        A[CursorController]
        B[KeyboardController]
    end

    subgraph PlatformSpecific
        C[CursorSimulator]
        D[MouseObserver]
        G[KeyboardEvent]
        I[KeyboardLayout]
    end

    subgraph External
        E[Operating System]
        F[Remote Events]
    end

    A --> C
    A --> D
    B --> G
    B --> I

    F --> RemoteControlEngine
    PlatformSpecific --> E

    %% Styling
    classDef engine fill:#4fc3f7,stroke:#0277bd,stroke-width:2px,color:#000
    classDef platform fill:#81c784,stroke:#388e3c,stroke-width:2px,color:#000
    classDef external fill:#ffb74d,stroke:#f57c00,stroke-width:2px,color:#000

    class A,B engine
    class C,D,G,I platform
    class E,F external

    %% Subgraph styling
    style RemoteControlEngine fill:#e1f5fe,stroke:#0277bd,stroke-width:3px
    style PlatformSpecific fill:#e8f5e8,stroke:#388e3c,stroke-width:3px
    style External fill:#fff3e0,stroke:#f57c00,stroke-width:3px
```

The remote control engine consist of two components:

- `CursorController`: Handles mouse and keyboard input from controllers.
- `KeyboardController`: Handles keyboard input from controllers.

Each component owns platform specific components which are using the platform specific apis.

#### CursorController

`CursorController` manages multi-user cursor interaction and visual feedback.

**Core Responsibility:**

- Coordinate control switching between sharer and controllers.
- Manage virtual cursor rendering on the overlay window.
- Handle input simulation through platform-specific components.

**Control Logic:**

- Only one cursor can have physical control at a time (OS limitation).
- Click or scroll events trigger control transfer to that cursor.
- Non-controlling cursors appear as virtual overlays.

**Events:**

- The movement and events of the controller cursor are arriving to the core process through the
  `WebRTC` data channel, then they are converted to `UserEvents` and forwarded to the cursor controller.
- The sharer's position is tracked by the mouse observer and broadcasted to the controllers via the
  `WebRTC` data channel.

**Platform Components:**

- **`MouseObserver`**: Captures local sharer mouse movements.
- **`CursorSimulator`**: Injects controller input into the `OS`.

Here follows a sequence diagram of the cursor controller:

```mermaid
sequenceDiagram
    participant LK as LiveKit Server
    participant RS as RoomService
    participant EL as Event Loop
    participant CC as CursorController
    participant GC as GraphicsContext
    participant OS as Operating System

    Note over LK,OS: Controller Input Processing

    LK->>RS: Controller input event
    RS->>EL: Convert to UserEvent
    EL->>CC: Process controller input
    CC->>GC: Update controller cursor position
    CC->>OS: Simulate mouse/keyboard input

    Note over OS,LK: Sharer Position Tracking

    OS-->>CC: Capture sharer mouse movement
    CC-->>EL: Report sharer position
    EL-->>RS: Publish sharer location
    RS-->>LK: Send position to controllers

    %% Styling
    %%{init: {
        'theme': 'base',
        'themeVariables': {
            'primaryColor': '#e1f5fe',
            'primaryTextColor': '#000',
            'primaryBorderColor': '#0277bd',
            'lineColor': '#424242',
            'secondaryColor': '#e8f5e8',
            'tertiaryColor': '#fff3e0'
        }
    }}%%
```

#### KeyboardController

High-level controller for keyboard input simulation across platforms. The `KeyboardController`
orchestrates keyboard simulation by managing layout detection, key mapping, and event generation.

**Core Responsibility:**

- Automatically handle layout changes and rebuild key mapping tables.
- Provide simple interface for simulating keystrokes from high-level keystroke data.

**Platform Components:**

- **`KeyboardLayout`**: Detects layout changes and translates keycodes to characters.
- **`KeyboardEvent`**: Generates platform-specific keyboard events for the `OS`.

### Screen Capture

```mermaid
graph TD
    subgraph CaptureEngine
        A[Capturer]
        B[Stream]
    end

    subgraph PlatformSpecific
        C[DesktopCapturer]
        D[NativeVideoSource]
        G[ScreenshareFunctions]
    end

    subgraph External
        E[Operating System]
        F[LiveKit Server]
    end

    A --> B
    B --> C
    B --> D
    A --> G

    D --> F
    C --> E

    %% Styling
    classDef engine fill:#4fc3f7,stroke:#0277bd,stroke-width:2px,color:#000
    classDef platform fill:#81c784,stroke:#388e3c,stroke-width:2px,color:#000
    classDef external fill:#ffb74d,stroke:#f57c00,stroke-width:2px,color:#000

    class A,B engine
    class C,D,G platform
    class E,F,H external

    %% Subgraph styling
    style CaptureEngine fill:#e1f5fe,stroke:#0277bd,stroke-width:3px
    style PlatformSpecific fill:#e8f5e8,stroke:#388e3c,stroke-width:3px
    style External fill:#fff3e0,stroke:#f57c00,stroke-width:3px
```

The `Capturer` manages the screen capture lifecycle and coordinates with `LiveKit` for real-time streaming.

**Core Responsibility:**

- Start/stop screen sharing sessions and manage capture streams.
- Generate thumbnails for content selection UI.
- Handle error recovery through automatic stream restart.
- Coordinate with `RoomService` for buffer sharing.

For platform-agnostic screen capturing, we use the `DesktopCapturer` object from our `LiveKit` fork
(we have modified `LiveKit` to expose libwebrtc's `DesktopCapturer`).

In the capture callback, which is called by the capturing thread, we process the captured buffer
and share it with `LiveKit`.

The platform-specific trait we have introduced is `ScreenshareFunctions`, which is used for
accessing the monitor ID. This requires different handling on each platform when using `winit`.

Here follows a sequence diagram of the capture engine:

```mermaid
sequenceDiagram
    participant TA as Tauri App
    participant SC as Socket Communication
    participant EL as Event Loop
    participant CA as Capturer
    participant DC as DesktopCapturer
    participant LK as LiveKit Server

    Note over TA,LK: Screen Sharing Session

    TA->>SC: Start screen share command
    SC->>EL: Convert to UserEvent
    EL->>CA: Initialize capture session
    CA->>DC: Start frame capture

    loop Frame Streaming
        DC->>LK: Capture and send frame
    end

    %% Styling
    %%{init: {
        'theme': 'base',
        'themeVariables': {
            'primaryColor': '#4fc3f7',
            'primaryTextColor': '#000',
            'primaryBorderColor': '#0277bd',
            'lineColor': '#424242',
            'secondaryColor': '#81c784',
            'tertiaryColor': '#ffb74d',
            'background': '#ffffff',
            'mainBkg': '#e1f5fe'
        }
    }}%%
```
