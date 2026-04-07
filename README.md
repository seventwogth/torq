# torq

Desktop control panel and runtime supervisor for launching Tor, monitoring bootstrap progress, and managing ControlPort features (including **NEWNYM**).

## What this repository contains

`torq` is a Rust + Tauri workspace with three main parts:

- **Desktop shell (`src-tauri`)**: exposes Tauri commands/events, persists runtime settings, and owns a single shared `TorManager` instance.
- **Runtime engine (`crates/torq-runtime`)**: starts/stops Tor, tails logs, tracks bootstrap/state, and optionally talks to ControlPort.
- **Frontend (`frontend`)**: Svelte UI for runtime controls, live status, activity feed, and settings.

## Workspace layout

```text
.
├── Cargo.toml                  # Rust workspace manifest
├── crates/
│   ├── torq-core/              # Shared runtime domain types (state/events/errors)
│   └── torq-runtime/           # Tor process + control supervision
├── frontend/                   # Svelte + Vite + Tauri API UI
├── src-tauri/                  # Desktop backend (Tauri host)
└── scripts/
    └── mock-tor.cmd            # Fake tor process for smoke tests
```

## Architecture

See `ARCHITECTURE.md` for a short overview of the runtime engine, desktop shell, and frontend, and how `TorManager` supervises the Tor process and ControlPort.

## Security model

See `SECURITY.md` for the high-level threat model, trust boundaries, ControlPort authentication considerations, and recommendations for securing configuration, logs, and the Tor binary.

## Prerequisites

### Required

- **Rust toolchain** (stable)
- **Node.js + npm**
- A Tor executable (or the included mock script for local smoke testing)

### Platform notes

Root `package.json` scripts currently use `npm.cmd` and Windows-style paths, so they are **Windows-oriented**.
On non-Windows systems, use direct `frontend` scripts and Rust commands shown below.

## Quick start (desktop app)

### 1) Install frontend dependencies

From repository root:

```powershell
npm run setup
```

Equivalent direct command (cross-platform-friendly):

```bash
npm install --prefix frontend
```

### 2) Run the desktop app in dev mode

From repository root:

```powershell
npm run tauri:dev
```

This starts Vite (`http://127.0.0.1:1420`) and the Tauri desktop host.

## Build commands

From repository root:

- `npm run build` — build frontend only.
- `npm run tauri:build` — build desktop app.
- `npm run check` — run frontend checks + `cargo fmt --all --check` + `cargo test --workspace` + `cargo clippy --workspace --all-targets --locked -- -D warnings`.

If you are not on Windows, run these directly instead:

```bash
npm --prefix frontend run check
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets --locked -- -D warnings
```

## Desktop backend API (Tauri)

### Invoke commands

- `tor_state`
- `tor_runtime_snapshot`
- `tor_start`
- `tor_stop`
- `tor_restart`
- `tor_new_identity`
- `get_runtime_config`
- `set_runtime_config`

### Emitted events

- `tor://state`
- `tor://runtime-snapshot`
- `tor://activity`

The frontend subscribes to all three for live updates.

## Runtime configuration

The desktop backend persists runtime config to:

- `%APPDATA%\torq\torq.config.json` on Windows when `APPDATA` is available.
- `$XDG_CONFIG_HOME/torq/torq.config.json` when `XDG_CONFIG_HOME` is set.
- `$HOME/.config/torq/torq.config.json` when `HOME` is available.
- `./torq.config.json` fallback in current working directory when none of the above are available.

### Bootstrap defaults

If no config file exists, backend defaults are derived from:

- `TORQ_TOR_EXE` (fallback: `tor.exe`)
- `TORQ_TOR_LOG` (fallback: `tor.log`)

### Config update behavior

`set_runtime_config` updates runtime + file persistence atomically (with rollback on persistence failure).
Runtime config changes are rejected while runtime status is `starting` or `running`; edit settings when Tor is stopped.

## `torq-runtime` CLI (smoke test)

You can run the runtime engine directly without the desktop shell:

```powershell
cargo run -p torq-runtime -- path\to\tor.exe .\tor.log
```

Interactive commands:

- `start`
- `stop`
- `restart`
- `newnym`
- `state`
- `exit` / `quit`

### CLI options

```text
cargo run -p torq-runtime -- [tor-path] [log-path] [--working-dir DIR] [--no-managed-log] [-- extra tor args]
```

- `--working-dir DIR` — sets Tor process working directory.
- `--no-managed-log` — external log mode; runtime only tails an existing log file.
- `--` — everything after is passed to Tor as extra args.

### Local mock smoke test (no Tor install)

```powershell
cargo run -p torq-runtime -- cmd.exe .\tor.log -- /C scripts\mock-tor.cmd
```

## Frontend notes

- Theme toggle (dark/light) is persisted in `localStorage` (`torq-theme`).
- Settings panel allows editing runtime paths, log mode, torrc usage, working directory, control settings, and timeouts.
- Activity feed coalesces bootstrap messages and keeps recent history.

## Key files

- Root workspace manifest: `Cargo.toml`
- Desktop backend: `src-tauri/src/lib.rs`
- Runtime config DTO + validation: `src-tauri/src/runtime_config.rs`
- Runtime engine entrypoint: `crates/torq-runtime/src/main.rs`
- Frontend app shell: `frontend/src/App.svelte`
- Tauri API client in frontend: `frontend/src/lib/torq-api.ts`
- Tauri app config: `src-tauri/tauri.conf.json`
