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
5. **Test ads only.** Never commit a configuration that loads live ads, and never
   click ad creatives in automated tests. Headless tests only; no test opens an
   app window.

## Setup

```sh
# Rust (rustup users get the pinned toolchain from rust-toolchain.toml)
cargo check --workspace -j 4

# JavaScript
npm install
```

Linux additionally needs the WebKitGTK 4.1 development packages; see the
`apt-get` line in `.github/workflows/ci.yml`.

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
  runtime (`tauri::test`).

### TypeScript standards

- `strict` plus `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`.
- No `any`, except at a typed boundary (for example a value received from IPC
  before validation), with a justified inline disable.
- TSDoc on every export; TypeDoc treats warnings as errors.
- Tests use Vitest and `@tauri-apps/api/mocks` instead of a real Tauri host.

### Lint exceptions

An `#[allow(...)]`, `#[expect(...)]` or `eslint-disable` must be as narrow as
possible and carry a comment that says why. Prefer `#[expect]` over `#[allow]`
so a stale exception fails the build.

## Dependencies

- Latest stable releases only; no alpha, beta or rc versions. Shared Rust
  versions live in `[workspace.dependencies]`.
- Overwolf npm packages track the `latest` dist-tag, never `beta` or `next`.
- Tauri must stay at 2.12.0 or newer (GHSA-w28w-mhc8-qvjv binds channel data
  to the webview that requested it; ow-tauri places remote pages next to app
  webviews).
- TypeScript stays on the newest release that typescript-eslint and TypeDoc
  support (currently 6.0).

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(plugin): ...`, `fix(renderer): ...`, `docs(contract): ...`,
  `chore(ci): ...`. Scopes: `plugin`, `main`, `electron`, `renderer`, `example`,
  `docs`, `contract`, `adr`, `ci`, `deps`.
- Small, focused commits. Update `CHANGELOG.md` under `[Unreleased]`.
- Changes to the example that differ from upstream are listed in
  `examples/packages-sample/CHANGES-FROM-UPSTREAM.md`.
