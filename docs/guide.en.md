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

The **Windows floating window** fits into one line: work status, current model, and average speed. A highlighted “Reset” marker appears for new important reset messages. Click to expand details; drag its left side to reposition it.

Disable the floating window in Settings if desired. Left-click the tray icon to open the same detail panel; right-click for Settings, Quit, and other simple actions.

On **macOS**, click the menu bar icon to open details. Both platforms share the features below.

## Settings and refresh

- **Appearance:** dark, light, or system theme. Blue is the default accent; violet, teal, amber, and rose are also available.
- **Independent refresh:** configure automatic or manual refresh and intervals separately for account limits and Codex Resets messages, or refresh immediately. Manual mode shows cached data at startup and makes no background requests for that group. Translation runs only when clicked.
- **On-demand collection:** on Windows, automatic account requests require expanded, visible details; the compact window makes no account requests. macOS runs once at startup, then on schedule while the panel is visible. Reopening refreshes only when the cache is due. Local log collection and public message refresh run independently.

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
- [Windows parity and cross-platform release](development/windows-release-0.1.6.md) · [macOS validation](development/macos-2026-10-06.md)
- [Development handoff](development/handoff-2026-10-06.md) · [Reference prices and coverage](../resources/pricing/README.md)
