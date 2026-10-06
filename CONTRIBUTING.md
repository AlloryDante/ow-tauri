# Contributing to ow-tauri

Thanks for helping. This file is the rulebook; please read it before opening a
pull request.

## Ground rules

1. **The contract comes first.** [docs/CONTRACT.md](docs/CONTRACT.md) is the
   spec the Rust plugin and the JS package both code against. A change to a
   command, event, payload or public JS member updates the contract in the same
   pull request.
2. **Decisions are recorded.** A change that reverses or extends an
   architecture decision adds or supersedes an ADR in [docs/adr/](docs/adr/).
3. **Sources.** Behaviour we copy from ow-electron must come from public
   sources: Overwolf's documentation, the published typings, the official
   sample, the published JavaScript of `@overwolf/ow-electron-builder`, or
   behaviour you can observe on a machine running an ow-electron app (files it
   writes, requests visible in a proxy). If a behaviour cannot be traced to one
   of these, implement it only as an explicit, documented, configurable option
   and add it to [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md).
4. **Vendor neutral.** No app-specific names, ids or branding in code, docs,
   fixtures or commit messages. Use the upstream sample's identity in examples.
   Game fixtures and scenarios use well-known public game ids that are already
   in Overwolf's public game-events status data (for example 5426); adding
   another title needs a reviewer's agreement.
5. **Ads in tests and labs** ([ADR 0005](docs/adr/0005-ads-test-live-parity.md)).
   Test ads (`--test-ad`) are the default for every test and lab run, and no
   committed configuration, script or fixture enables live ads. A lab run that
   must show live fill may load live ads, at most 10 loads per run, each one
   logged. Never click an ad or send input to an ad guest, and never show a
   window: windows stay hidden (or at alpha 0 inside the screen bounds when an
   ad must fill), with the dock icon hidden on macOS. Automated tests are
   headless.

## Setup

```sh
# Rust (rustup users get the pinned toolchain from rust-toolchain.toml)
cargo check --workspace -j 4

# JavaScript (the lockfile is committed; CI uses npm ci)
npm ci --workspace ow-tauri --include-workspace-root
```

Linux additionally needs Tauri's prerequisites (WebKitGTK 4.1, libssl,
libxdo and friends); see the `apt-get` line in `.github/workflows/ci.yml`.

## Quality gates

CI runs all of these on macOS, Windows and Linux. Run them locally before you
push:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings
cargo test --workspace -j 4
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps -j 4

npm run format:check
npm run lint
npm run typecheck
npm test
npm run docs
```

CI also runs cargo-deny, `npm audit`, a check that every `@overwolf/*`
package is on its `latest` dist-tag (`node scripts/check-overwolf-dist-tags.mjs`),
an MSRV build, and, weekly, the newest stable Rust with the newest tauri 2.x
and a main-webview liveness soak. CI also rebuilds
`crates/tauri-plugin-overwolf/js/` and fails if the committed output differs:
run `npm run build:injected --workspace ow-tauri` and commit the result
whenever you change `packages/ow-tauri/src/{bootstrap,guest}`. A release
build of the crate fails while a script is missing; a debug build embeds a
placeholder and prints a warning. A job that builds the crate in release mode
without running the app sets `OW_TAURI_ALLOW_MISSING_JS=1`
([CONTRACT A.1](docs/CONTRACT.md#a1-configuration)).

Keep heavy jobs serial on small machines: `-j 4` for cargo, two Vitest workers
(set in `vitest.config.ts`).

### Rust standards

- `#![deny(missing_docs)]`; every public item has rustdoc, and public functions
  have a runnable example where one makes sense.
- Clippy pedantic is on for the workspace and CI denies warnings.
- Errors are typed with `thiserror`; every fallible public function documents
  its errors (`# Errors`).
- No `unwrap()`, `expect()` or `panic!` outside tests. Every `unsafe` block has
  a `// SAFETY:` comment.
- Unit tests sit next to the code; IPC and permission tests use Tauri's mock
  runtime (`tauri::test`). The mock runtime fires no window, navigation,
  page-load or exit-request events; the crate's `test-util` feature exposes
  hidden `Overwolf::test_*` hooks that drive those handlers. The permission matrix covers every command against
  every webview class, including a child webview inside a `bw-*` window and a
  remote page in a `bw-*` window; the router tests include property tests for
  ordering (CONTRACT C.3).

### TypeScript standards

- `strict` plus `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`.
- No `any`, except at a typed boundary (for example a value received from IPC
  before validation), with a justified inline disable.
- TSDoc on every export; TypeDoc treats warnings as errors.
- Tests use Vitest and `@tauri-apps/api/mocks` instead of a real Tauri host.
  The bootstrap and guest scripts are tested the same way (happy-dom).
- A bundler fixture builds a CommonJS `require('electron')` through webpack
  with the alias, so the package's export conditions stay compatible.

### Lint exceptions

An `#[allow(...)]`, `#[expect(...)]` or `eslint-disable` must be as narrow as
possible and carry a comment that says why. Prefer `#[expect]` over `#[allow]`
so a stale exception fails the build.

## Dependencies

- Latest stable releases only; no alpha, beta or rc versions. Shared Rust
  versions live in `[workspace.dependencies]`.
- Overwolf npm packages track the `latest` dist-tag, never `beta` or `next`;
  CI enforces it.
- Tauri must stay at 2.12.1 or newer. GHSA-w28w-mhc8-qvjv (fixed in 2.11.6
  and 2.12.0): Tauri's channel-data fetch command skipped the ACL, so queued
  channel payloads and large invoke responses could be fetched by other
  webviews; ow-tauri sends all host traffic over channels and places remote
  pages next to app webviews.
- TypeScript stays on the newest release that typescript-eslint and TypeDoc
  support (currently 6.0).
- `@types/node` follows the minimum supported Node major (22), not the newest.
- Rust: the toolchain pin (`rust-toolchain.toml`) matches the reference
  development compiler so local and CI lint results agree; the weekly CI job
  covers the newest stable.
- Third-party GitHub Actions are pinned by commit SHA; Dependabot proposes
  updates for Cargo, npm and Actions weekly.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(plugin): ...`, `fix(renderer): ...`, `docs(contract): ...`,
  `chore(ci): ...`. Scopes: `plugin`, `main`, `electron`, `renderer`, `example`,
  `docs`, `contract`, `adr`, `ci`, `deps`.
- Small, focused commits. Update `CHANGELOG.md` under `[Unreleased]`.
- Changes to the example that differ from upstream are listed in
  `examples/packages-sample/CHANGES-FROM-UPSTREAM.md`.

## Documentation checklist

The project is not complete until each of these exists and is current. The
final review before a release checks the list.

| Document | Content | Status |
|---|---|---|
| `README.md` | overview, quick start, requirements | present |
| `docs/ARCHITECTURE.md` | components, process model, flows, security design | present |
| `docs/CONTRACT.md` | wire, JS API, IPC, guest shim, analytics, state, manifest, package runtime, updater | present |
| `docs/adr/` | decision records | present |
| `docs/OPEN-QUESTIONS.md` | questions for Overwolf with interim behaviour | present |
| `docs/PORT-MAP.md` | the sample's port, file by file | present |
| `docs/MIGRATION.md` | step-by-step guide: pre-flight audit (Node built-ins and Electron-only libraries, each with its replacement), bundler recipes (webpack, Vite, esbuild), typings (`tsconfig` paths), network (`fetch` and CORS, scoped HTTP), sync-to-async checklist, capability and CSP templates, debugging `ow-main`, what changes for users; full Electron to ow-tauri mapping tables | **to write** |
| `docs/api/` | reference per area (main, electron, renderer, plugin Rust API): hand-written overviews that link to the generated references. TypeDoc writes to `packages/ow-tauri/docs-out/` (`npm run docs`) and rustdoc to `target/doc/`; both are generated, git-ignored and never committed | **to write** |
| `examples/packages-sample/CHANGES-FROM-UPSTREAM.md` | every change against upstream `8a27053` | **to write** (with the port) |
| `examples/packages-sample/.env.example` | dev-mode variable names, no values | **to write** (with the port) |
| `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE` | | present |

The package runtime interface for Overwolf has no separate guide while
packages are out of scope: its design is
[CONTRACT Appendix P](docs/CONTRACT.md#appendix-p-deferred-design-package-runtime-interface).
