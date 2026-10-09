# Usage and development

[Back to README](../README.en.md) · [中文](guide.md)

## Install and open

| Platform                              | Download                       | Getting started                                                                        |
| ------------------------------------- | ------------------------------ | -------------------------------------------------------------------------------------- |
| **Windows 10 / 11 · x64**             | `x64-setup.exe` from Releases  | Install and launch from Start. Click the tray icon or floating window to open details. |
| **macOS 12+ · Apple Silicon / Intel** | Universal `.dmg` from Releases | Drag into Applications, launch, and click the menu bar icon.                           |

Windows requires WebView2 Runtime; the installer can install it when needed. Login startup is off by default and can be enabled in Settings.

The Windows installer has no Authenticode signature. The macOS package is ad-hoc signed without Developer ID signing or notarization.

## Everyday use

The **Windows floating window** fits into one line: work status, current model, and average speed. A highlighted “Reset” marker appears for new important reset messages. Click to expand details; drag anywhere on the strip to reposition it. Drag the expanded panel by its header; action buttons keep their normal behavior.

Disable the floating window in Settings if desired. Left-click the tray icon to open the same detail panel; right-click for Settings, Quit, and other simple actions.

Right-click the compact window for Expand, Quit, and Hide. Use the tray icon to reopen it after hiding. Messages in the current snapshot are automatically marked as read when the Tibo page is visible, including new messages refreshed while it remains open.

The **macOS menu bar** shows busy / idle / unknown, the current model, and average speed. `↻` marks new important reset announcements or plans, `•` marks other posts, and `!` marks source errors. Hover for details and click to open the panel. Parallel tasks show a count; long model names are shortened. Missing activity or speed samples display `—`. Local status keeps updating while the panel is hidden. Both platforms share the features below.


**Per-model run speed:** rows show the weighted average of the latest 50 valid runs within the last 30 days. Expand for the last 50 runs, 100 runs, or 30 days, plotted against device local time. Total output divided by total run duration excludes session idle time but may include reasoning, tools, and waits within runs. Current samples show logged modes such as Fast or Standard; missing modes stay Unknown and combined modes show Mixed. Hover or focus for time, speed, and mode. Old logs are backfilled locally in bounded batches, without extra network requests.

## Settings and refresh

- **Appearance:** dark, light, or system theme. Blue is the default accent; violet, teal, amber, and rose are also available.
- **Windows 10 glass:** uses an opaque background to avoid slow window dragging. The glass switch is disabled, and previously enabled glass settings are turned off automatically.
- **Independent refresh:** configure automatic or manual refresh and intervals separately for account limits and Codex Resets messages, or refresh immediately. Manual mode shows cached data at startup and makes no background requests for that group. Translation runs only when clicked.
- **On-demand collection:** on Windows, automatic account requests require expanded, visible details; the compact window makes no account requests. macOS runs once at startup, then on schedule while the panel is visible. Reopening refreshes only when the cache is due. Local log collection and public message refresh run independently.

## App updates

The app checks once in the background at startup. After that, only **Settings → App updates → Check for updates** makes a request. Opening, hiding, or recreating the panel reads cached state; failed checks do not retry automatically. Concurrent clicks share a request and respect the one-minute minimum interval and server retry delays.

New versions download automatically and are signature-verified. Click **Restart and update** to install and relaunch. In-app upgrades require 0.1.15 or newer; older versions need one manual installation. No gh or GitHub login is required. Checks are independent of account limits, Resets messages, and local collection.

## Data and metrics

- **Scope:** local statistics cover readable Codex logs; account lifetime totals come from the server and are labeled separately. Subagents are excluded from session lists and counts; their tokens and costs remain in total usage.
- **Cost and speed:** API-equivalent costs are reference-price estimates with explicit coverage, **not subscription charges**. `tok/s` is a sampled turn average including reasoning, tools, and waiting. Unavailable values display `—`.
- **Local data:** the usage ledger stays on-device; conversation bodies are not stored. Profile data stays in memory. Requests use file-based Codex credentials, never overwrite `auth.json`, and exclude credentials from the database and diagnostics.

## Development

Requires Rust stable ≥ 1.90, Node.js 24+, and pnpm 11. Release checks use Rust 1.97.1. Windows needs Visual Studio C++ tools and WebView2; macOS needs Xcode Command Line Tools.

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Check and build:

```sh
pnpm check
node --test apps/desktop/test/*.test.ts
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm build
```

Universal macOS package:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm build --target universal-apple-darwin
```

Cargo and target libraries must use the same Rust toolchain. Artifacts are generated under `target/universal-apple-darwin/release/bundle`.

See [screenshot notes](screenshots/README.md) for capture instructions.

## Development documentation

Implementation documents are currently in Chinese:

- [Product and technical design](design/codexpulse-v1.md) · [Implementation status](development/status.md)
- [Latest cross-platform release](development/release-0.1.7.md) · [Windows parity baseline](development/windows-release-0.1.6.md) · [macOS validation](development/macos-2026-10-06.md)
- [Development handoff](development/handoff-2026-10-06.md) · [Reference prices and coverage](../resources/pricing/README.md)
