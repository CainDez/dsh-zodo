# A0 locked host facts

Status: in progress. This file is append-only evidence for the A0 harness lock; update it when a
fact is re-measured, and keep the original archive hash.

## Host release

| Fact | Value | Measurement |
|---|---|---|
| Product | DeepSeek Harness Desktop | installed application metadata |
| Version | `0.2.1-rc.3` | `resources/runtime/runtime.json`, executable file version |
| Build commit | `be72761ee1a6736ebcdf4f3739363b083ae53c98` | `app.asar/package.json` `dshBuildCommit` |
| Source tag | `dsh-v0.2.1-rc.3` | `git ls-remote` |
| Source commit | `1f81c047fc48fa190a683e469daa699857e9afe8` | shallow clone of the tag |
| Source commit date | `2026-10-23T11:29:36+08:00` | `git log -1` |
| License | MIT | `LICENSE` in the application and upstream repository |
| Electron/Node | Electron reports Node `v24.21.0` via `ELECTRON_RUN_AS_NODE=1` | exact command in evidence index |
| App archive | `C:\Users\<user>\AppData\Local\Programs\DeepSeek Harness\resources\app.asar` | 182,434,783 bytes |
| App archive SHA-256 | measured in `EVIDENCE_INDEX.md` | `Get-FileHash -Algorithm SHA256` |
| Plugin install profile | isolated `zodo-a0` profile under the A0 runtime home | created from shipped `web` template |
| Bridge | Host webserver same-origin BFF + native tool to a versioned Rust probe | A0 ADR |
| Core protocol | `zodo-a0-bridge/1` | probe handshake |

## Why this bridge

The installed release publishes Cordis plugins, web-client slots, native tools, `ctx.webServer`, and
the generated Typert Remote machinery. It does not publish the Typert generator as an installable
SDK. A0 therefore uses the host's public `ctx.webServer` route surface for the React-to-Host RPC and
the public native tool registry for agent-to-Host calls. Both paths enter the same Host adapter and
the same versioned Rust probe. This is a typed, same-origin JSON RPC boundary owned by Zodo; it does
not claim to be the generated `ctx.remote.zodo` namespace. The generated Typert boundary is revisited
during A2 and, if adopted, gets its own generated-artifact evidence rather than being asserted.

Trust model: the browser route is loopback-only, same-origin, and gated by an HttpOnly same-site
session cookie minted by the route itself. The native tool carries a separate Host-agent identity.
The UI session cannot be obtained by a model. A same-UID process can still inspect files and
credentials; that limitation is recorded rather than papered over.

## Clean-profile procedure

1. Set `DSH_HOME` to an isolated A0 directory (never the desktop profile).
2. `dsh --profile zodo-a0 --from-default-profile web --dump-config` to materialize the shipped web
   composition.
3. Build the probe and plugin; install the bundle with `dsh plugin --profile zodo-a0 add <path>`.
4. Start the host on a dedicated loopback port.
5. Exercise the native tool from a real host session and the React tab from the served page.
6. Stop and restart the host, reload/unload the plugin, and repeat with a second client process.

## Measured A0 result

The clean-profile run is recorded in `implementation/EVIDENCE_INDEX.md` (A0-0001 to A0-0008) and
`artifacts/evidence/a0/`.

- The plugin installed from a directory outside the Harness repository into the isolated `zodo-a0`
  profile; the boot graph contains `@zodo/harness-plugin` and the guide card opened a real Sidebar tab.
- The React tab minted an HttpOnly UI cookie, called `/zodo-bridge/v1/rpc`, and received `pong`,
  an echo, and an explicit `CANCELLED` result for a cancelled 15-second operation.
- A real local Ollama model (`aravhawk/qwen3.5-opus-4.6:latest`) emitted `zodo_probe({"method":"ping"})`;
  the host adapter invoked the Rust probe; the durable session log records
  `tool/call` and `tool/result` with `{"ok":true,"protocol":"zodo-a0-bridge/1","message":"pong"}`.
- A second probe process with the same state directory detected the healthy instance and exited
  without binding a second server.
- A forced host stop ended the host-owned probe child; restarting the host created a new probe
  instance on first use. A live externally owned probe is left running by the adapter. The
  distinction is deliberate and recorded in `adapterOwned` behavior.

Open A0 gaps: none for the tested uninstall/restart lifecycle. Hot reload without a Host restart is
still not a Host capability; uninstall was exercised live and the profile was restored before restart.

- The incompatible-artifact H22 arm is verified: the adapter runs a `--version` preflight before
  bind; replacing the packaged Core with the A0 probe produced `CORE_INCOMPATIBLE` at
  `/zodo-bridge/v1/health` (503) and left no second Core bound. See
  `artifacts/evidence/a0/h22-incompatible.json` and `implementation/EVIDENCE_INDEX.md` A0-0010.
- Hot uninstall was exercised from the Host plugin manager while the Host stayed alive: plugin card/tab
  disappeared, bridge health returned 404, independent Core PID 42664 stayed alive at r68. The profile
  was restored with pnpm and the Host restarted; see `a0/hot-uninstall.png` and A0-0011.

## Host model selection (AI-12, documented unsupported)

Harness can select its own chat model in the Host UI. Zodo deliberately does **not** map that host
selector to the Core `ModelGateway` route: the Host model selector is a separate data flow from the
Core model route, and no host-model override is injected into Core. This is the documented unsupported
arm of spec/06 §2 rather than a silently ignored control. Core model configuration remains the only
authority for Core AI runs (`model.json`, policy, local/cloud routing).

The bridge is not the real Zodo Core. A1 starts the independent Rust application and SQLite facts;
A2 replaces this probe path with the real KernelClient.
