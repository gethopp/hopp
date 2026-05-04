---
title: Self-Hosting
description: Run your own Hopp server with PostgreSQL, Redis, LiveKit, and Caddy auto-TLS.
---

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
git clone --depth 1 https://github.com/gethopp/hopp.git
cd hopp/selfhost
```

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

### 4. Verify

```bash
curl https://hopp.example.com/api/health
# → OK
```

### 5. Create the first account

Open `https://hopp.example.com` in a browser and sign up. The first registered account becomes the team admin owner.

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

## Web app and desktop

The web app served by your backend works without a rebuild — it derives the API URL from `window.location` at runtime.

For the desktop app, you can set a custom backend URL at runtime via **Settings > Custom Backend URL** (see [Settings docs](/features/settings)).

## Advanced

### Custom ports

Copy `compose.override.example.yml` to `compose.override.yml` and adjust port mappings.

### Behind your own reverse proxy

If you already run nginx/traefik/caddy, skip the bundled Caddy:

1. Remove the `caddy` service from `compose.yml` (or use an override)
2. Point your proxy to `backend:1926` (Docker DNS) and `127.0.0.1:7880` (LiveKit on host networking)
3. Set `USE_TLS=false` (already the default)
4. Set `LIVEKIT_SERVER_URL=wss://livekit.yourdomain.com` in `.env`

## Local development (running selfhost stack on your machine)

To run the self-hosted stack against `localhost` instead of a public domain:

### 1. Generate localhost TLS certs

The backend serves TLS directly when `USE_TLS=true`. Generate certs from the `backend/` folder:

```bash
cd ../backend
task create-certs   # uses mkcert; installs the local CA on first run
```

This produces `backend/certs/localhost.pem` and `backend/certs/localhost-key.pem`, which the override mounts into the backend container.

### 2. Use the bundled local override

A ready-made `compose.override.yml` is committed for local use. It:

- exposes `backend:1926` directly on the host (skipping Caddy)
- enables `USE_TLS=true` and mounts `../backend/certs`
- swaps LiveKit to `livekit.dev.yaml` (narrow UDP range `50000-50100`, avoids macOS port-conflict storms)

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
