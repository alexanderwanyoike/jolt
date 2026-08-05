# tauri-plugin-jolt

Tauri v2 plugin exposing the local [Jolt](https://github.com/alexanderwanyoike/jolt)
daemon to app webviews through audited proxy commands, so Jolt desktop apps
need no hand-written Rust wire code.

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_jolt::init())
    // ...
```

Grant the capability in `capabilities/default.json`:

```json
{ "permissions": ["jolt:default"] }
```

Pair it with [`@jolt/sdk`](https://github.com/alexanderwanyoike/jolt/tree/main/sdks/js)
on the webview side:

```ts
import { TauriTransport } from "@jolt/sdk/transport-tauri";
const transport = new TauriTransport({ plugin: true });
```

The daemon base URL defaults to `http://127.0.0.1:9862` (override with
`JOLT_DAEMON_URL`). Only the daemon's `/app/v1` and `/api/v1` surfaces are
reachable; every session-scoped call still carries the app's bearer token, so
the plugin adds no authority beyond what the user approved in Jolt Console.
