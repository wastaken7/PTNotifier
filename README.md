# PTNotifier (`ptn`)

Private tracker notification and private message (PM) monitor written in Rust, featuring an embedded WebUI, SQLite persistence, and multi-channel alert delivery.

---

## Features

- **Embedded WebUI**: manage trackers, notification channels, global settings, notification history, and live logs (via Server-Sent Events at `/api/events`).
- **Security & Internet-Ready Auth**:
  - Account-based authentication with Argon2id password hashing, `HttpOnly`/`SameSite=Lax`/`Secure` session cookies, CSRF token verification, IP rate limiting, and brute-force account lockout.
  - AES-256-GCM encryption at rest for tracker cookies, API tokens, and notification channel secrets (`PTN_MASTER_KEY` or auto-generated key file).
  - Secret redaction in WebUI/API/SSE responses, SSRF URL & DNS validation, strict Content-Security-Policy, and token-authenticated RSS feeds (`/rss?token=...`).
  - Binds to `127.0.0.1:8085` by default for safe local use.
- **Background Scheduler**: periodically polls enabled trackers while respecting the global request delay (`request_delay_secs`) and per-tracker minimum intervals.
- **Flexible Authentication**: accepts Netscape/Mozilla `cookies.txt` format or raw `Cookie: key=value; ...` headers, as well as `api_token` where supported.
- **Filtering & Actions**:
  - Per-tracker `ignore_patterns` list.
  - Optional `mark_as_read` setting (global or per-tracker override).
  - Optional error/session-expiry alerts (`send_error_notifications`).
- **4 Notification Channels**: **Discord**, **Telegram**, **Gotify**, and **ntfy**, with per-channel toggles for notifications, PMs, and errors, plus test delivery via WebUI or CLI.
- **RSS 2.0 Feed**: Subscribe at `/rss` (or `/api/rss`) to receive captured notifications, private messages, and error alerts in any RSS reader. Supports optional query filters `tracker_id`, `item_type` (`notification`, `message`, `error`), and `limit` (up to `500`).
- **Local Persistence**: stores configuration, tracker state, and notification history in a single SQLite database (`ptnotifier.db` by default).

---

## Supported Trackers

Built-in presets cover major tracker engines as well as individual sites:

| Engine / Network | Trackers / Presets |
| :--- | :--- |
| **UNIT3D** | UNIT3D (Generic), Hawke-Uno (Deep Space) |
| **AvistaZ Network** | AvistaZ, CinemaZ, PrivateHD, ExoticaZ, AnimeZ |
| **NexusPHP** | NexusPHP (Generic), BTSCHOOL, Baozi, Lajidui, LongPT, March, PTCafe, PTFans, PTGTK, PTSKit, RailgunPT |
| **Gazelle** | GreatPosterWall, Orpheus *(API token + optional cookies)*, Anthelion *(API token only)* |
| **XBTIT** | HD-Space, HD-Torrents, SportsCult |
| **Other** | BJ-Share *(3600s minimum interval)*, BrasilTracker, Amigos Share Club (ASC), BTArg, DigitalCore, Fappaizuri, HDCiTY, IPTorrents, ImmortalSeed, SceneTime, TorrentDay, TorrentLeech, YGG Reborn |

---

## Notification Channels

- **Discord** (Webhook with rich embeds and configurable character limit)
- **Telegram** (Bot API with HTML formatting)
- **Gotify**
- **ntfy**

---

## Installation & Running

### Docker

Multi-arch images (`linux/amd64` and `linux/arm64`) are published to GitHub Container Registry:

```bash
docker run -d \
  --name ptnotifier \
  -p 127.0.0.1:8085:8085 \
  -v $(pwd)/data:/data \
  -e PTN_ADMIN_USER=admin \
  -e PTN_ADMIN_PASSWORD="change-this-strong-password" \
  -e PTN_MASTER_KEY="$(openssl rand -hex 32)" \
  -e PTN_TRUST_PROXY=true \
  --restart unless-stopped \
  ghcr.io/wastaken7/ptnotifier:latest
```

Or build locally from the `Dockerfile`:

```bash
docker build -t ptnotifier .
docker run -d -p 127.0.0.1:8085:8085 -v $(pwd)/data:/data ptnotifier
```

By default, the container runs:
```text
ptn serve --db /data/ptnotifier.db --bind 0.0.0.0:8085
```
If `PTN_ADMIN_PASSWORD` is not set on first run, a one-time setup token is printed to the container logs for creating the administrator account via the WebUI.

### Building from Source

Requires Rust (2024 edition; the `Dockerfile` uses `rust:1.85-bookworm`):

```bash
cargo build --release
./target/release/ptn serve
```

Open the WebUI at `http://127.0.0.1:8085`.

### Exposing to the Internet

When exposing PTNotifier publicly:
1. Run behind an HTTPS reverse proxy (Caddy, Nginx, Traefik, or Cloudflare Tunnel) and enable `--trust-proxy` (or `PTN_TRUST_PROXY=true`) so client IPs and `Secure` cookies work accurately.
2. Bind `ptn` locally (`--bind 127.0.0.1:8085`, the default) and forward from the reverse proxy.
3. Set `PTN_MASTER_KEY` (64 hex characters / 32 random bytes) in your environment so database secret encryption does not rely on a key file on disk.
4. Generate a dedicated RSS token in the **Security** tab for external RSS readers (`/rss?token=...`).
5. If Gotify or ntfy runs on your local LAN/Docker network, pass `--allow-private-networks` (or `PTN_ALLOW_PRIVATE_NETWORKS=true`).

---

## CLI Usage

```text
ptn [OPTIONS] [COMMAND]
```

### Global Options

| Flag | Default | Description |
| :--- | :--- | :--- |
| `--db <PATH>` | OS data dir (`ptnotifier.db`) | Path to the SQLite database file (`PTN_DB`). |
| `--log-level <LEVEL>` | `info` | Log level filter (`error`, `warn`, `info`, `debug`, `trace`) (`PTN_LOG_LEVEL`). |

### Subcommands

- **`serve`** — Run the background scheduler and embedded WebUI/API server.
  ```bash
  ptn serve --bind 127.0.0.1:8085
  ptn serve --bind 0.0.0.0:8085 --trust-proxy
  ptn serve --tray
  ```
- **`tray`** *(default when running `ptn` with no arguments on Windows and macOS)* — Run the background scheduler and WebUI with a system tray icon (alias for `serve --tray`). On Linux, build with `cargo build --release --features tray` (requires `libgtk-3-dev` and `libayatana-appindicator3-dev`).
  ```bash
  ptn
  ptn tray
  ```
- **`user`** — Manage WebUI accounts from the CLI (`create`, `passwd`, `list`, `delete`).
  ```bash
  ptn user create --username admin --role admin
  ptn user passwd --username admin
  ptn user list
  ```
- **`check`** — Run a one-shot check for all enabled trackers, or a specific tracker by ID or name.
  ```bash
  ptn check
  ptn check --tracker "TorrentLeech"
  ```
- **`test-notify`** — Send a test notification to all enabled channels, or a specific channel by ID or name.
  ```bash
  ptn test-notify
  ptn test-notify --channel "Discord"
  ```

---

## Global Settings (WebUI)

These settings can be configured through the WebUI (`/api/settings`):

| Setting | Default | Description |
| :--- | :--- | :--- |
| `default_check_interval_secs` | `1800` | Default polling interval in seconds. |
| `request_delay_secs` | `5.0` | Minimum delay between HTTP requests. |
| `http_timeout_secs` | `30.0` | HTTP request timeout in seconds (effective minimum 5s). |
| `mark_as_read` | `false` | Mark notifications/PMs as read after processing. |
| `send_error_notifications` | `false` | Send alerts to configured channels on tracker errors or expired cookies. |
| `discord_embed_limit` | `4096` | Character limit for Discord embeds. |
