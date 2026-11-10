# Contributing

Thanks for taking a look at Claude Switchboard.

## Before you open a PR

- **Typos, docs, small fixes** — just send the PR, no need to open an issue first or discuss branching.
- **New features or behavior changes** — open an issue first so we can agree on the approach before you invest the time.

## Local setup

```bash
pnpm install
pnpm tauri dev
```

## Before you submit

Run the same checks CI runs:

```bash
pnpm lint   # tsc --noEmit
pnpm test   # vitest

cd src-tauri
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

All four must pass. `test.yml` runs the full matrix — frontend and Rust — on macOS, Windows, and Linux, even though this is a desktop app with no Linux build target: it's cheap insurance against platform-`cfg`-gated Rust code that only breaks on the OS where it compiles.

## Previewing the UI in a browser

The real UI can be opened in a normal browser with fixture data, without building the app. This is the quickest way to check a visual change at different data volumes and in both themes.

```bash
pnpm preview:ui
# then open http://localhost:1430/preview.html
```

| Parameter | Values | Default |
|---|---|---|
| `scale` | `heavy` (300 projects, 136 days, about $13,000 a month), `light`, `empty` | `heavy` |
| `theme` | `light`, `dark` | `light` |
| `view` | `expanded` (960×640), `compact` (360×380) | `expanded` |
| `delay` | milliseconds to hold back report queries, to see loading states | `0` |

For example `http://localhost:1430/preview.html?scale=empty&theme=dark`. Every Tauri command is answered from `src/preview/fixtures.ts`; a command the preview does not know logs a `[preview] unmocked command` warning in the console. The preview is not part of the production bundle. It cannot show what only the packaged app does: window dragging, resizing, launching a session.

## Building a release bundle

The status line bridge (`src-tauri/statusline-bridge`, binary `switchboard-statusline`) ships inside the app as a sidecar. `pnpm tauri dev` doesn't need it; a bundle does, and it is built with its own `bridge` Cargo profile (unwinding panics, so a failure in its observation step can never change your status line). Use the bundle config, which builds the bridge first and adds it as an `externalBin`:

```bash
pnpm bundle                       # tauri build --config src-tauri/tauri.bundle.conf.json
# local macOS build without updater artifacts:
pnpm tauri build --bundles app --config src-tauri/tauri.bundle.conf.json \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Check that `Contents/MacOS/switchboard-statusline` is in the `.app`. A plain `pnpm tauri build` (without the bundle config) produces an app without it, and the switch in Settings will report that the bridge couldn't be installed.

The Windows half of the bridge (`shell_windows.rs`) is pure logic tested everywhere plus a `cfg(windows)` spawn that has not been run on Windows. To type-check it from macOS: `rustup target add x86_64-pc-windows-msvc && cargo clippy -p statusline-bridge --target x86_64-pc-windows-msvc --all-targets -- -D warnings`.

## Scope

This is a two-platform desktop app (macOS + Windows) built on Tauri v2. Please keep PRs focused — one behavior change per PR is easier to review than a bundle of unrelated fixes.
