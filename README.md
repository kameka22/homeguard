# HomeGuard

Centralized website blocking for Windows/Linux machines on a home network,
managed from a macOS desktop app.

A lightweight agent runs as a system service on each machine and enforces
blocking through the `hosts` file — schedules, temporary unblocks,
self-healing if tampered with, and neutralization of DNS-over-HTTPS
bypass. The macOS app never runs anything in the background on the agents'
behalf: it only talks to them when launched or on explicit action (push
model, no polling).

> Personal project, built for family use. Full, living architecture
> document: [PLAN.md](./PLAN.md). The app's UI and `PLAN.md` are in French;
> this README and all code comments are in English.

## Features

- **Reusable domain lists**, assigned to a machine or a group of machines,
  with automatic expansion of common variants (`www.`, `m.`, alternate
  domains).
- **Time-based scheduling** per assignment (weekly grid), evaluated in a
  fixed timezone on the agent side — never the system timezone, to prevent
  bypass.
- **Temporary unblocks** (15 min → end of day), removed automatically on
  expiry even if the app is closed.
- **Self-healing**: the `hosts` file is watched, any tampering is corrected
  and logged.
- **DNS-over-HTTPS neutralization** (Chrome, Edge, Brave, Chromium, Firefox)
  to prevent bypassing the block via DoH.
- **Automatic discovery** of machines on the local network (mDNS) — no IP to
  type in by hand.
- **One-line deployment** (`curl | sudo sh` / `irm | iex`), agent binary
  bundled with the app, checksum-verified (SHA-256).
- **Remote agent updates**, with post-restart verification (per machine or as
  a single batch action).
- **Real online/offline status** per machine, not just "seen once at some
  point".
- **Dashboard**: detected drift, upcoming schedule transitions, unified app +
  agent history.
- **Encrypted export / import** of the entire configuration (including the
  CA), to migrate to another Mac without re-enrolling any machine.
- **Security**: end-to-end mTLS, self-generated CA stored in the macOS
  Keychain, an agent's private key never leaves its own machine.

## Architecture

```
┌────────────── Mac (Tauri app) ───────────────┐
│  UI (Svelte)                                 │
│  Rust backend                                │
│   ├─ PKI (rcgen) ── CA + client cert         │──► macOS Keychain
│   ├─ mDNS discovery (mdns-sd)                │
│   ├─ mTLS client (reqwest + rustls)          │
│   ├─ Policy engine                           │
│   ├─ Sync / history                          │
│   ├─ Enrollment server (axum, temporary)     │
│   └─ Local storage (SQLite)                  │
└───────────────┬──────────────────────────────┘
                │  mTLS :7878         ▲ mDNS _homeguard._tcp
                ▼                     │
┌──────── Windows / Linux machine ────┴────────┐
│  homeguard-agent (SYSTEM/root service)       │
│   ├─ HTTPS API (axum + rustls, mTLS)         │
│   ├─ mDNS announce                           │
│   ├─ Policy evaluator                        │
│   ├─ hosts block writer + DNS flush          │
│   └─ Local event log                         │
└──────────────────────────────────────────────┘
```

The full diagram, threat model and per-flow details live in
[PLAN.md](./PLAN.md).

## Repository layout

```
homeguard/
├── crates/
│   ├── common/      # shared models: policy, hosts parsing, PKI, wire types
│   ├── agent/       # agent binary (run / enroll / install / uninstall), Windows/Linux service
│   └── app-core/    # app-side logic: SQLite, policy engine, mTLS client, mDNS, HTTP servers
├── app/             # Tauri application (macOS only today, see Project status)
│   ├── src/         # Svelte frontend
│   └── src-tauri/   # Rust backend, commands exposed to the UI
├── scripts/         # build-agents.sh, install.sh, install.ps1
└── PLAN.md          # architecture and progress document
```

## Requirements

- **macOS** (the controlling app is macOS-only today — Windows/Linux support
  for the app, and a macOS build of the agent, are planned; see
  [PLAN.md](./PLAN.md))
- [Rust](https://rustup.rs/) (edition 2021, stable toolchain)
- [Node.js](https://nodejs.org/) + npm
- To cross-compile the Windows/Linux agent binaries from the Mac:
  [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild), `zig` and
  `mingw-w64` (see the header of
  [`scripts/build-agents.sh`](./scripts/build-agents.sh) for the exact
  install commands)

## Getting started

```bash
git clone git@github.com:kameka22/homeguard.git
cd homeguard

# Cross-compiles the agent binaries (Linux x86_64/arm64, Windows x86_64) and
# stages them in app/src-tauri/resources/agents/ — needed before the app's
# first launch for the one-line installer to actually work.
./scripts/build-agents.sh

cd app
npm install

# Development mode (hot reload)
npm run tauri dev
```

## Production build

```bash
cd app
npm run tauri build
```

Produces a `.app` and a `.dmg` under `app/src-tauri/target/release/bundle/`.

## Tests

```bash
# From the repo root — covers common, agent, app-core
cargo test --workspace
```

## Usage

> The UI is currently French-only (`Postes` = machines/hosts, `+ Postes` =
> add a machine). Labels below match what's actually on screen.

1. Launch HomeGuard on the Mac; on first launch, a local CA is generated and
   stored in the Keychain.
2. From the **Postes** screen, click **+ Postes**: a command to copy shows
   up, with the IP, token and CA fingerprint already filled in.
3. Run that command on the machine to monitor (administrator PowerShell on
   Windows, `sudo sh` on Linux). The agent downloads, verifies its own
   integrity, enrolls with the app, and installs itself as a system service.
4. The machine shows up automatically in the app (mDNS discovery).

## Project status

Functionally complete on the code side. Built and tested primarily on
Linux/macOS; the Windows service side compiles (cross-compilation verified)
but hasn't been validated on real Windows hardware yet.

| Area | Status |
|---|---|
| Policy model, PKI, hosts parsing (`common`) | ✅ tested |
| Agent core (API, self-healing, anti-DoH, Linux service) | ✅ tested |
| Agent — Windows service (SCM + dispatcher) | 🟡 compiles, not validated on real hardware |
| Enrollment and assisted PKI | ✅ tested |
| Tauri app (macOS, Keychain, mDNS, UI) | ✅ verified on macOS |
| Deployment (install scripts, cross-compilation) | ✅ verified |
| Scheduling, temporary unblocks | ✅ tested |
| Integrity and anti-DoH on Windows | 🟡 written, not verified on real hardware |
| Remote agent updates | ✅ tested (mechanism), not exercised in the field |
| Encrypted export / import | ✅ tested |
| macOS code signing and notarization | ⬜ not started |
| UI translation (English) | ⬜ not started — UI is French-only today |
| Cross-platform app (Windows/Linux) + macOS agent | ⬜ not started — `app-core` is already platform-agnostic Rust; the macOS-specific bits (Keychain storage, service management, browser policy) still need Windows/Linux/macOS counterparts respectively |

Full section-by-section detail: [PLAN.md](./PLAN.md).

## Security and limitations

- Every connection between the app and an agent uses mTLS: each side only
  talks to a peer presenting a certificate signed by the local CA.
- Each agent's private key is generated on its own machine and never leaves
  it.
- Blocking acts on the `hosts` file and the browser policies listed above; it
  doesn't cover VPNs, proxies, or unmanaged portable browsers.
- Install scripts are served over plain HTTP on the local network
  (attackable by another party on the same LAN) — binary integrity is
  checked via SHA-256 and enrollment is pinned by CA fingerprint, which
  remains acceptable on a trusted home network.

## License

[MIT](./LICENSE)
