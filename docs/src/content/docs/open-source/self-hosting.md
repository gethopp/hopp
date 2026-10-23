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

## Restricting sign-up

By default anyone who can reach your server can create an account, and an account created without a team invite gets a team of its own. If your server is reachable from the internet, you can require a team invite link to sign up.

In the **web app** (not the desktop app), sign in as the instance owner, open **Settings**, and turn on **Require an invite to sign up** under **Instance**. It is off by default, so existing installations are unaffected.

- Anyone without a valid invite link is turned away at sign-up, for email/password and social login alike.
- Existing accounts can still sign in. Joining your team through its invite link still works.
- The setting is changed from the web interface only; there is no environment variable.

### Who is the instance owner?

Hopp has no instance-wide admin role: "admin" only means admin of your own team, and everyone who signs up without an invite is an admin of a team of their own. So only the **instance owner** can change this setting, defined as the **earliest account that still exists**. On a new server, **create your own account first, before sharing the URL**, so that you are the owner. Ownership cannot be transferred, and it passes to the next-earliest account if the owner's account is deleted.

To see who the owner is:

```bash
docker compose exec db psql -U hopp hopp -c "select email, created_at from users order by created_at, id limit 1;"
```

If you lock yourself out, you can switch the setting off directly:

```bash
docker compose exec db psql -U hopp hopp -c "update instance_settings set signup_requires_invite = false;"
```

### What an invite link does and does not protect

This setting is only as strong as team invite links:

- Every team has one invite link, and **any member of that team** (not just admins) can see it.
- Invite links **do not expire** and cannot be rotated or revoked. Someone who has been removed from a team keeps the link they already had.
- A link from **any** team on the server lets someone sign up, not just yours. If your server was open for a while, review the existing accounts first (for example with `select email, team_id from users;`), because anyone who signed up earlier has a link of their own.

If the first account on a server with no accounts at all (for example after a database reset) is created while this setting is on, it is allowed without an invite, otherwise no account could ever be created.

## Telemetry

**Web app**: telemetry (PostHog analytics) is disabled by default in selfhosted builds. No data is sent to external services.

**Desktop app**: when connecting to a selfhosted backend, disable telemetry from the desktop app **Settings**.

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
