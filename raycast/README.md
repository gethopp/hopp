# Hopp for Raycast

Start a call with a teammate or join a room in [Hopp](https://gethopp.app) without leaving Raycast.

## Commands

| Command           | What it does                                                                                                               |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------- |
| **Call Teammate** | Lists your teammates with their live status. Rings them in Hopp — or joins their ongoing call when they're already in one. |
| **Join Room**     | Lists your team's rooms with live participant counts and joins the selected room.                                          |

## How it works

The extension never talks to LiveKit directly. It combines two existing pieces of the desktop app:

1. **App state** — the macOS app persists its session token and (for self-hosted setups) its custom
   server URL in `~/Library/Application Support/com.hopp.app/app_state.json`. The extension reads it to
   call the Hopp API for teammates, rooms, and presence, so there is no separate sign-in.
2. **Deep links** — the app registers the `hopp://` URL scheme. The extension opens:

   | Deep link                    | Action in the app                             |
   | ---------------------------- | --------------------------------------------- |
   | `hopp:///call/{userId}`      | Ring a teammate (same as the "Call" button)   |
   | `hopp:///join-call/{userId}` | Join a teammate's ongoing call                |
   | `hopp:///room/{roomId}`      | Join a named room (same as the "Join" button) |

   Handlers live in `tauri/src/lib/deepLinkUtils.ts`.

If the app isn't running yet, macOS launches it and the deep link is delivered once it is ready.

## Requirements

- macOS with the [Hopp desktop app](https://github.com/gethopp/hopp/releases/latest) installed and signed in.
- [Raycast](https://www.raycast.com).

## Preferences

- **Server URL** — optional override of the backend URL. Leave empty to use the server configured in
  the app (defaults to the Hopp cloud at `pair.gethopp.app`).

## Development

```bash
cd raycast
npm install
npm run dev
```

`npm run dev` runs `ray develop`, which installs the extension into Raycast with live reload.
`npm run build` produces a production build via `ray build`.
