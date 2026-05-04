# Hopp self-hosted quickstart

Run your own Hopp server. Includes PostgreSQL, Redis, LiveKit (WebRTC), and Caddy (auto-TLS).

## Prerequisites

- A server with Docker and Docker Compose installed
- A public hostname for your server. Either:
  - **A real domain** with A records for `<DOMAIN>` and `livekit.<DOMAIN>` (or wildcard `*.<DOMAIN>`) pointing to your server's IP.
  - **Or `sslip.io`** for quick testing — use `<your-ip>.sslip.io` as `DOMAIN` (e.g. `51.15.42.10.sslip.io`). Resolves automatically, no DNS setup. Subdomains work (`livekit.<your-ip>.sslip.io`). Real domain recommended for production.
- Open firewall ports (see [Firewall](#firewall) below)

## Quickstart (6 steps)

### 1. Clone the repo

```bash
git clone -b v0.0.X --depth 1 https://github.com/gethopp/hopp.git
cd hopp/selfhost
```

Replace `v0.0.X` with the latest release tag.

### 2. Configure environment

```bash
cp .env.example .env
```

Edit `.env` and set at minimum:

```env
DOMAIN=hopp.example.com
SESSION_SECRET=<openssl rand -base64 32>
LIVEKIT_API_KEY=<openssl rand -base64 32>
LIVEKIT_API_SECRET=<openssl rand -base64 64>
POSTGRES_PASSWORD=<openssl rand -base64 24>
```

Generate secrets with the `openssl` commands shown.

### 3. Start the stack

```bash
docker compose up -d
```

### 4. Wait for TLS certificate

```bash
docker compose logs caddy | grep "certificate obtained"
```

This usually takes 10-30 seconds.

### 5. Verify

```bash
curl https://hopp.example.com/api/health
# → OK
```

### 6. Create the first account

Open `https://{DOMAIN}` in a browser and sign up. The first registered
account becomes the workspace owner.

## Firewall

Many cloud providers (Scaleway DEV, Hetzner Cloud, basic DigitalOcean droplets) do **not** apply a firewall by default — these ports will already be reachable. Skip this section unless your provider has a security group, network ACL, or you've enabled `ufw`/`firewalld` on the host.

> AWS, GCP, Azure, and Scaleway PRO/PROD instances typically need explicit security-group rules.

Open these ports if your provider applies a firewall:

| Port        | Protocol | Purpose              |
| ----------- | -------- | -------------------- |
| 80          | TCP      | HTTP (TLS challenge) |
| 443         | TCP      | HTTPS                |
| 7881        | TCP      | LiveKit TCP fallback |
| 50000-60000 | UDP      | WebRTC media         |

Caddy handles TLS termination on ports 80/443. LiveKit media bypasses Caddy and goes directly to the server.

## Email

Email is disabled by default. Set `RESEND_API_KEY` in `.env` to enable password reset, welcome, and invitation emails. [Get a free key at resend.com](https://resend.com).

Generic SMTP support is [a good first issue](https://github.com/gethopp/hopp/issues) — contributions welcome.

## Web app and desktop

The web app served by your backend works without a rebuild — it derives the
API URL from `window.location` at runtime.

The desktop app bakes the server URL at build time. To point official binaries at your server, you need to rebuild:

```bash
cd tauri
VITE_API_BASE_URL=https://hopp.example.com yarn build
```

A runtime server URL setting (`Settings > Server URL`) is planned for a future release.

## Updating

```bash
docker compose pull
docker compose up -d
```

Pin your version by setting `HOPP_VERSION=v0.0.X` in `.env`.

## Advanced

### Custom ports

Copy `compose.override.example.yml` to `compose.override.yml` and adjust port mappings.

### Behind your own reverse proxy

If you already run nginx/traefik/caddy, skip the bundled Caddy:

1. Remove the `caddy` service from `compose.yml` (or use an override)
2. Point your proxy to `backend:1926` (Docker DNS) and `127.0.0.1:7880` (LiveKit on host networking)
3. Set `USE_TLS=false` (already the default)
4. Set `LIVEKIT_SERVER_URL=wss://livekit.yourdomain.com` in `.env`

### BYO LiveKit Cloud

Replace the `livekit` service with your LiveKit Cloud URL in `.env`:

```env
LIVEKIT_SERVER_URL=wss://your-project.livekit.cloud
```

Then remove the `livekit` service from compose.

## Troubleshooting

| Symptom                    | Fix                                                             |
| -------------------------- | --------------------------------------------------------------- |
| Caddy fails to obtain cert | Check DNS points to server, port 80 is open                     |
| Backend won't start        | Check `docker compose logs backend`, verify `.env` values       |
| Screen share fails         | Verify firewall ports 7881 (TCP) and 50000-60000 (UDP) are open |
| Password reset not working | Set `RESEND_API_KEY` in `.env` — email is disabled without it   |

## Local development (running selfhost stack on your machine)

To run the self-hosted stack against `localhost` instead of a public domain:

### 1. Generate localhost TLS certs

The backend serves TLS directly when `USE_TLS=true`. Generate certs from the
`backend/` folder:

```bash
cd ../backend
task create-certs   # uses mkcert; installs the local CA on first run
```

This produces `backend/certs/localhost.pem` and `backend/certs/localhost-key.pem`,
which the override mounts into the backend container.

### 2. Use the bundled local override

A ready-made `compose.override.yml` is committed for local use. It:

- exposes `backend:1926` directly on the host (skipping Caddy)
- enables `USE_TLS=true` and mounts `../backend/certs`
- swaps LiveKit to `livekit.dev.yaml` (narrow UDP range `50000-50100`, avoids
  macOS port-conflict storms)

Set `.env`:

```env
DOMAIN=localhost
LIVEKIT_SERVER_URL=ws://localhost:7880
USE_TLS=true
```

Then:

```bash
docker compose up -d db cache livekit backend
# (skip caddy — not needed for localhost)
```

Backend reachable at `https://localhost:1926`.

### 3. LiveKit port forwarding (macOS / Docker Desktop)

Docker Desktop on macOS does not forward UDP ranges efficiently. The narrowed
range `50000-50100` (in `livekit.dev.yaml`) keeps Docker startup fast.
For real WebRTC sessions over the public internet, use the production
config (`livekit.yaml`) which uses the full `50000-60000` range.
