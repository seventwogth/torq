## Threat model (high level)

`torq` is a **local desktop application** that:

- launches and supervises a Tor process,
- stores Tor runtime configuration on disk,
- optionally talks to Tor's ControlPort over TCP.

The primary security goal is to avoid:

- exposing sensitive configuration (paths, working directory, ControlPort settings),
- weakening Tor's own security properties,
- enabling privilege escalation or arbitrary code execution beyond starting Tor itself.

Out of scope for `torq`:

- hardening Tor itself — Tor remains a separate, external binary.
- network-level anonymity guarantees — those are provided by Tor, not by `torq`.

## Trust boundaries

- **Frontend ↔ Desktop shell (Tauri)**:
  - All privileged operations are behind Tauri commands.
  - The frontend cannot start arbitrary processes; it can only issue the predefined Tor commands.
- **Desktop shell ↔ Runtime engine (`torq-runtime`)**:
  - Tauri owns `TorManager` and only exposes a constrained API.
  - `TorManager` only starts the configured Tor binary with configured arguments.
- **Runtime engine ↔ Tor process**:
  - `TorProcess` starts/stops a single child process.
  - `LogTail` and `TorControlClient` observe Tor output and state but do not modify Tor binaries or the filesystem beyond reading logs and optional cookie files.

## Configuration and persistence

- Runtime configuration is persisted to a per-user location:
  - `%APPDATA%\torq\torq.config.json` on Windows (when available),
  - `$XDG_CONFIG_HOME/torq/torq.config.json` or `$HOME/.config/torq/torq.config.json` on Unix-like systems,
  - a local `./torq.config.json` fallback.
- The config file contains:
  - paths to the Tor executable and log file,
  - Tor working directory and extra arguments,
  - ControlPort host/port and authentication settings.
- The file is **not encrypted**; it should be protected by OS-level user permissions.

## ControlPort and authentication

When ControlPort is configured:

- `TorControlClient` supports:
  - **NULL auth** (no authentication),
  - **cookie auth** (Tor's control cookie file).
- Cookie authentication:
  - reads a Tor-generated cookie file from a path configured in `TorRuntimeConfig`,
  - enforces expected length (32 bytes),
  - sends hex-encoded cookie contents over the ControlPort connection.
- ControlPort is assumed to be bound to a **loopback** interface (e.g., `127.0.0.1`), as configured by the user in Tor itself.

Recommended configuration:

- Keep ControlPort bound to localhost.
- Prefer cookie authentication when available.
- Restrict filesystem permissions on the Tor data directory and ControlPort cookie file so other local users cannot read them.

## Process execution model

- `TorProcess` is responsible for spawning a **single Tor child process** using:
  - a configured executable path,
  - an optional working directory,
  - additional arguments supplied by the user.
- `TorManager` validates that the Tor path is non-empty but does not attempt to sandbox or validate the Tor binary itself.
- There is no in-process plugin or scripting system; Tor is the only external process `torq` is intended to execute.

Implications:

- Users should ensure the configured Tor executable is trustworthy and not replaced by a malicious binary.
- On multi-user systems, OS-level mechanisms (file ACLs, package manager integrity checks) should be used to protect the Tor binary.

## Logging and telemetry

- `torq` does **not** send telemetry or analytics.
- Logs are produced by:
  - the Tor process itself (to a log file or stdout, depending on configuration),
  - the desktop application runtime (standard Rust/Tauri logs, not shipped externally).
- Log paths are configurable; they may contain:
  - Tor operational details,
  - error messages and bootstrap progress.

Recommendations:

- Treat logs as potentially sensitive and protect them with OS-level file permissions.
- Avoid sharing logs publicly without redaction.

## Updates and dependencies

- `torq` depends on:
  - the Rust ecosystem (Tokio, Tauri, etc.),
  - the Node.js ecosystem for the frontend.
- Dependency updates should be reviewed and pinned via the standard Rust/Node tooling.

Security hygiene:

- Run `cargo audit` or similar tools periodically to detect vulnerable Rust crates.
- Keep Tor, Tauri, and other dependencies reasonably up to date.

