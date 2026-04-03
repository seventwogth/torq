# torq
A control tool for Tor routing, bridge management, and traffic orchestration.

## Development

The canonical desktop development entrypoint is the repository root.

1. Install frontend dependencies once:

```powershell
npm run setup
```

2. Start the desktop app in dev mode from the repository root:

```powershell
npm run tauri:dev
```

Root-level scripts:

- `npm run tauri:dev` starts the Tauri desktop app and the Vite dev server.
- `npm run build` builds the frontend only.
- `npm run tauri:build` builds the desktop app.
- `npm run check` runs frontend checks plus `cargo fmt --all --check`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.

Useful environment variables:

- `TORQ_TOR_EXE` points the desktop/runtime backend at a Tor binary. If unset, the app tries `tor.exe`.
- `TORQ_TOR_LOG` overrides the runtime log path. If unset, the app uses `tor.log` in the repo root.

Tauri config lives in [src-tauri/tauri.conf.json](/C:/Users/stargazer/github/torq/src-tauri/tauri.conf.json).
Frontend sources live in [frontend](/C:/Users/stargazer/github/torq/frontend).

## Runtime config

The desktop backend stores runtime configuration as JSON in `torq.config.json`.
By default it uses a user-level config path such as `%APPDATA%\torq\torq.config.json` on Windows, with a current-directory fallback if no user config root is available.

On startup the backend loads config from that file into a small in-memory config store.
If the file is missing, it falls back to the bootstrap defaults from `TORQ_TOR_EXE` / `TORQ_TOR_LOG` (or `tor.exe` / `tor.log` when those env vars are unset).

Configuration updates are persisted through the backend API and are only accepted while Tor is not active.
If runtime is in `Starting` or `Running`, `set_runtime_config` is rejected instead of hot-reloading the process.

## Runtime CLI smoke test

Run the CLI example from the workspace root:

```powershell
cargo run -p torq-runtime -- path\to\tor.exe .\tor.log
```

You can also set `TORQ_TOR_EXE` instead of passing the path as the first argument.

The CLI is an interactive runtime smoke-test. Type `start`, `stop`, `restart`,
`newnym`, `state`, or `quit` in stdin.

For a local smoke test without a real Tor install:

```powershell
cargo run -p torq-runtime -- cmd.exe .\tor.log -- /C scripts\mock-tor.cmd
```
