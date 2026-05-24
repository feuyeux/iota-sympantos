# iota-desktop

Tauri desktop workbench for iota-sympantos.

The desktop app is daemon-first:

- React renders the Chat-first workbench.
- Tauri commands connect to the local iota daemon.
- The daemon owns `EnginePool`, `IotaEngine`, ACP processes, approvals, config reads, and runtime events.
- `~/.i6/nimia.yaml` remains the only configuration source.

## Development

```bash
cd crates/iota-desktop
npm install
npm run tauri dev
```

## Verification

```bash
cargo test -p iota-core daemon
cargo test -p iota-desktop
cd crates/iota-desktop && npm test && npm run build
```
