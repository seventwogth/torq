# torq
A control tool for Tor routing, bridge management, and traffic orchestration.

## Runtime MVP

Run the CLI example from the workspace root:

```powershell
cargo run -p torq-runtime -- path\to\tor.exe .\tor.log
```

You can also set `TORQ_TOR_EXE` instead of passing the path as the first argument.

While the process is running, type `stop`, `restart`, `newnym`, or `quit` in stdin.

For a local smoke test without a real Tor install:

```powershell
cargo run -p torq-runtime -- cmd.exe .\tor.log -- /C scripts\mock-tor.cmd
```
