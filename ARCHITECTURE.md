## Overview

`torq` is a desktop control panel and runtime supervisor for Tor. The workspace is split into:

- **Desktop shell (`src-tauri`)**: Tauri host written in Rust.
- **Runtime engine (`crates/torq-runtime`)**: Tor process supervisor and ControlPort client.
- **Shared core (`crates/torq-core`)**: domain types (state, events, errors).
- **Frontend (`frontend`)**: Svelte UI talking to Tauri commands and listening to events.

The data flow is intentionally one-directional: user interactions in the frontend are translated into Tauri commands, which call into `TorManager` in the runtime engine. `TorManager` owns the Tor process and emits domain events and state snapshots, which are sent back to the desktop shell and then to the frontend.

## Runtime engine (`torq-runtime`)

The runtime engine is responsible for:

- Spawning and stopping the Tor process (`TorProcess`).
- Managing runtime configuration (`TorRuntimeConfig`).
- Observing Tor bootstrap and status via:
  - log tailing (`LogTail`), and
  - optional Tor `ControlPort` (`TorControlClient`, `TorControlConfig`).
- Reducing events into state snapshots (`TorRuntimeSnapshotReducer`).

### TorManager

`TorManager` is the in-process API for controlling a single Tor runtime session. It owns:

- An mpsc channel for control commands (`TorCommand`).
- A watch channel for current Tor state (`TorState`).
- A watch channel for a richer runtime snapshot (`TorRuntimeSnapshot`).
- A watch channel for the current runtime config (`TorRuntimeConfig`).
- A broadcast channel for runtime events (`TorEvent`).

The `TorManager::new` constructor:

- Validates the initial `TorRuntimeConfig`.
- Spawns a long-lived **supervisor task** (`run_supervisor`) that listens for:
  - control commands (start/stop/restart/new identity),
  - config update requests, and
  - Tor process exit.
- Spawns a **state store task** (`run_state_store`) that:
  - consumes `TorEvent`s from a bounded queue,
  - applies them via `TorRuntimeSnapshotReducer`,
  - publishes:
    - `TorState` via `watch::Sender`,
    - `TorRuntimeSnapshot` via `watch::Sender`,
    - raw `TorEvent`s via a `broadcast::Sender`.

### RuntimeSession

`RuntimeSession` models a single Tor process lifetime:

- `TorProcess` — child process handle.
- `LogTail` — background task that polls and parses the log file.
- `TorControlClient` — optional ControlPort client, session-scoped.
- `BootstrapObserver` — optional background task polling bootstrap progress via ControlPort.

The supervisor creates a `RuntimeSession` on `TorCommand::Start` and tears it down on `TorCommand::Stop` or process exit. Session-scoped helpers are intentionally not shared globally so that ControlPort connections and bootstrap observers are correctly tied to the lifetime of the Tor process.

## Desktop shell (`src-tauri`)

The Tauri backend owns:

- A single `TorManager` instance that is shared across Tauri commands.
- A small DTO layer for persisting and loading runtime configuration.
- A mapping layer between:
  - Tauri commands (`tor_start`, `tor_stop`, `tor_restart`, `tor_new_identity`, etc.),
  - state/query commands (`tor_state`, `tor_runtime_snapshot`, `get_runtime_config`),
  - configuration mutation (`set_runtime_config`).

Key responsibilities:

- Persist runtime config to disk (platform-specific locations).
- Hydrate the in-memory runtime on startup using persisted config and env defaults.
- Bridge `TorEvent`s and snapshots to frontend events.

## Frontend (`frontend`)

The Svelte frontend is responsible for:

- Invoking Tauri commands for runtime control.
- Subscribing to `tor://state`, `tor://runtime-snapshot`, and `tor://activity` events.
- Rendering:
  - high-level Tor status,
  - bootstrap progress,
  - ControlPort availability,
  - activity feed and warnings/errors,
  - runtime configuration forms.

The frontend does not talk to Tor directly; all access is mediated by Tauri and `TorManager`.

## Process and state model

- At most **one** `TorManager` and **one** Tor process are active per desktop instance.
- Commands are serialized through the supervisor task and applied sequentially.
- State is:
  - **event-sourced** via `TorEvent` (activity feed),
  - **snapshotted** via `TorRuntimeSnapshot` (internal state machine),
  - **projected** to a simpler `TorState` for convenience and external consumers.

