# Contributing to ow-tauri

Thanks for helping. Please read this page before opening a pull request.

## Ground rules

1. The wire comes first. Overwolf must receive exactly what ow-electron
   sends: the same requests, headers, ids, cookies, state-file bytes and
   guest messages ([CONTRACT](docs/CONTRACT.md)), except the host label and
   the documented platform gaps (CONTRACT D.8.3). A change that could alter
   any of them needs a new parity run before it merges
   ([When a parity run is needed](#when-a-parity-run-is-needed)).
2. Use Tauri idioms. The plugin is a Tauri plugin. Use Tauri's own APIs,
   capabilities and events. ow-tauri adds no Electron API.
3. Record decisions. A change that reverses or extends an architecture
   decision adds or supersedes an ADR in [docs/adr/](docs/adr/README.md).
4. Copy observed behaviour only. Behaviour copied from ow-electron must come
   from public sources: Overwolf's documentation, the published typings,
   the official sample, or behaviour you can observe on a machine running
   an ow-electron app (requests, files written, cookies). If a behaviour
   cannot be traced to one of these, make it an explicit, documented option
   and add it to [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md).
5. Stay vendor neutral. No app names, personal names, real uids, machine ids
   or local user paths in code, docs, fixtures, images or commit messages.
   Examples use the placeholder identity in the repository. CI enforces
   this (the identity check below).
6. Use test ads only, and keep labs invisible. See [Lab rules](#lab-rules).

## Setup

You need:

- Rust: the pinned toolchain in `rust-toolchain.toml` (rustup picks it up);
  the minimum supported Rust version is 1.90.
- Node 22.12 or newer.
- On Linux, Tauri's prerequisites (WebKitGTK 4.1 and friends; see the
  `apt-get` step in `.github/workflows/ci.yml`).

```sh
cargo check --workspace -j 4
npm ci
```

The crate embeds the guest scripts and the global API script from
committed build output (`crates/tauri-plugin-overwolf/js/` and
`api-iife.js`). After you change `packages/guest-shims` or the API
package's entry, rebuild and commit the output:

```sh
npm run build:generated
```

CI fails when the committed output differs from a fresh build
(`npm run check:generated`).

Keep heavy jobs serial on small machines: `-j 4` for cargo, two Vitest
workers.

## Checks

A release needs every check below to pass. Your pull request needs the ones
CI runs (Rust, TypeScript, ACL suite, docs, identity and green `main`), plus
a parity run if you touch parity-sensitive code.

| Check | What it checks | Where it runs | Run it yourself |
|---|---|---|---|
| Rust | rustfmt, clippy (pedantic, warnings are errors), tests and doctests, rustdoc, MSRV 1.90 build, no `unstable` on Linux or in the build step | CI `rust`, `msrv` | see below |
| TypeScript | Prettier, ESLint (including "no Node imports" in the API package), typecheck, Vitest with coverage thresholds, TypeDoc, generated scripts match their sources, package checks (publint, are-the-types-wrong) | CI `node` | see below |
| ACL suite | every command against every webview class in Tauri's mock runtime (`crates/tauri-plugin-overwolf/tests/acl-app`) | CI `rust` (part of `cargo test`) | `cargo test --workspace` |
| macOS lab | the parity scenarios against an ow-electron twin, invisible (`everVisible: false`, never frontmost), plus the 53 macOS key-input cases | maintainers' Macs | [tools/parity-harness](tools/parity-harness/README.md) |
| Windows lab | the parity scenarios and the 59 Windows key-input cases | `windows-lab.yml`, on pushes to `main` that touch the plugin, the API, the guest shims or the harness, weekly, and on demand | ask a maintainer to dispatch it |
| Performance | idle cost of the 250 ms poll, CPU of one visible ad against the ow-electron twin, cold start to burst, macOS guest memory over a reload run | labs | PARITY lists the numbers |
| Release dry run | packaging and publishing without uploading | `release.yml` | [docs/RELEASING.md](docs/RELEASING.md) |
| Docs | every relative link and anchor in every Markdown file; rustdoc and TypeDoc without warnings | CI `docs-links`, `rust`, `node` | `npm run check:links` |
| Identity | no denied names or identities in any tracked file | CI `release-checks` | `node scripts/release/identity-check.mjs` |
| Examples | all four examples build on Windows, macOS and Linux | CI `example` | `npm run build` |
| Green `main` | `main` is green after every merge; a red `main` is fixed before new work lands | CI | watch your push |
| Full parity run | the macOS and Windows labs on the full scenario set against a fresh ow-electron `latest` baseline | labs | see below |

Local commands for the Rust, TypeScript and docs checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings
cargo test --workspace --all-features -j 4
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features -j 4

npm run format:check
npm run lint
npm run typecheck
npm run test:coverage
npm run docs
npm run check:generated
npm run check:packages
npm run check:links
```

### When a parity run is needed

A change to any of these re-runs the affected parity scenarios before it
merges, and the pull request links the result:

- `crates/tauri-plugin-overwolf/src/platform/`
- `crates/tauri-plugin-overwolf/src/host/ads.rs` and `src/host/ads/`
- `crates/tauri-plugin-overwolf/src/ads/`
- `crates/tauri-plugin-overwolf/src/analytics/`
- `crates/tauri-plugin-overwolf/src/consent/` and `src/host/consent.rs`
- `crates/tauri-plugin-overwolf/src/state/`
- `packages/guest-shims/`

`ADS_PARITY_ARGS` (the Windows ads browser arguments) is pinned by a unit
test. Changing it needs a full Windows lab run.

### Scheduled jobs and drift

- Every week, CI builds with the newest stable Rust and the newest Tauri
  2.x (`latest` job). A failure there means the `^2.12.1` range is no
  longer honest. Fix it or narrow the range in a patch release.
- Every week, the Windows lab (`windows-lab.yml`) also runs as the drift
  lab: against Overwolf's live ad page and the newest
  `@overwolf/ow-electron@latest`, with test ads only. If `__overwolf__`
  keys, message names or request shapes drift, the run fails with the diff.
- The maintainers own the drift lab. After a drift failure, a fix or a
  documented deviation in PARITY ships within 7 days.

## Lab rules

These rules protect users, advertisers and Overwolf's data. They apply to
every test, lab run, script and example.

1. Use test ads by default. Every test and lab run uses test ads
   (`ads.testAd`, `--test-ad` or `OW_TAURI_TEST_AD`). No committed config,
   script or fixture turns on live ads. A lab run that must prove live fill
   may load at most 10 live ads, each one logged.
2. Never click an ad. Never send input to an ad guest. Click tests use a
   loopback fixture page that replaces the ad page in test mode and opens
   nothing.
3. Never show a window. Lab apps run invisible:
   - with the `lab` feature, `OW_TAURI_LAB_INVISIBLE=1`;
   - as an accessory app on macOS, at alpha 0, ignoring the mouse and never
     focused.

   A macOS run must end with `everVisible: false` and must never have
   been frontmost. No screen captures. Automated tests are headless.
4. Keep example labs on loopback. The example lab runs send the plugin's
   analytics and its `cmp-eu-only` request to a loopback sink that the
   runner starts, never to Overwolf; the test ads and the consent page
   still load from Overwolf. The lab apps are built under a lab-only bundle
   identifier (ending in `.lab`), so their app data and single-instance
   lock stay apart from a developer's own copy of the app.
5. Use the placeholder identity. Committed files, shared captures and
   images use the placeholder identity. A registered app's identity for a
   lab run lives only in the git-ignored `local.identity.json` and never
   reaches a commit, an image or an issue.
6. Run one app at a time on small machines. Keep `-j 4`, and wait for a
   quiet machine before a lab run.

## Development features and the release guard

The crate's `lab` and `test-util` features are for development:

- `test-util` exposes hidden `Builder` hooks for Tauri's mock runtime.
- `lab` writes parity traces (only with `OW_TAURI_LAB_DIR` set at run time)
  and makes windows invisible.

Both are a `compile_error!` in an optimised build (`PROFILE=release`, or
any `opt-level` other than 0). The one escape hatch is for lab builds that
must run optimised, such as the Windows lab:

```sh
OW_TAURI_ALLOW_DEV_FEATURES_IN_RELEASE=1 cargo build --release --features lab
```

Use it for `cargo test --release` with `test-util` too. Never set it in a
build you ship.

## Code standards

### Rust

- `#![deny(missing_docs)]`. Every public item has rustdoc, and public
  functions have a runnable example where one makes sense.
- Clippy pedantic is on, and CI denies warnings.
- Errors use `thiserror`, and every fallible public function documents its
  errors (`# Errors`). The error codes JavaScript sees are fixed
  (CONTRACT A.4).
- No `unwrap()`, `expect()` or `panic!` outside tests. Every `unsafe` block
  has a `// SAFETY:` comment.
- Platform code reaches WebView2 and WKWebView through raw pointers and the
  crate's own bindings. It never names wry's platform types, so a new Tauri
  minor cannot break integrators.
- Never call `get_webview_window` in plugin code; use `compat::window` and
  `compat::webview`.
- IPC and permission tests use Tauri's mock runtime (`tauri::test`). The
  mock runtime fires no window, navigation, page-load or exit-request
  events; the `test-util` feature's hidden hooks drive those handlers.

### TypeScript

- `strict`, plus `noUncheckedIndexedAccess` and
  `exactOptionalPropertyTypes`.
- No `any`, except at a typed boundary (such as a value received from IPC
  before validation), with a justified inline disable.
- TSDoc on every export; TypeDoc treats warnings as errors.
- `tauri-plugin-overwolf-api` is browser-only: no Node imports.
- Tests use Vitest and `@tauri-apps/api/mocks`. Guest shims are tested the
  same way, with happy-dom.

### Lint exceptions

An `#[allow(...)]`, `#[expect(...)]` or `eslint-disable` is as narrow as
possible and says why. Prefer `#[expect]`, so a stale exception fails the
build.

## Dependencies

- Latest stable releases only; no alpha, beta or rc versions. Shared Rust
  versions live in `[workspace.dependencies]`.
- `tauri` stays at `^2.12.1`. That release includes the fix for
  GHSA-w28w-mhc8-qvjv (Tauri's channel-data fetch skipped the ACL).
- Overwolf npm packages (`@overwolf/*`) track the `latest` dist-tag, never
  `beta`, `next` or a pinned older line. CI checks it
  (`node scripts/check-overwolf-dist-tags.mjs`).
- Third-party GitHub Actions are pinned by commit SHA. Dependabot proposes
  updates weekly.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(plugin): …`, `fix(api): …`, `docs(contract): …`, `chore(ci): …`.
  Scopes: `plugin`, `api`, `cli`, `shims`, `build`, `updater`, `examples`,
  `docs`, `contract`, `adr`, `ci`, `deps`.
- Small, focused commits.
- A user-visible change adds a line to `CHANGELOG.md` under
  `[Unreleased]`.
- A change to a command, event, payload or config key updates CONTRACT,
  the API reference and CONFIG in the same pull request.

## Documentation map

| Document | For |
|---|---|
| [README.md](README.md) | evaluators: what it is, platforms, quick start |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | contributors: modules, lifecycle, guests, security mechanisms |
| [docs/CONTRACT.md](docs/CONTRACT.md) | maintainers and Overwolf: the wire contract |
| [docs/SECURITY.md](docs/SECURITY.md) | reviewers: threat model, permissions, what is written to disk |
| [docs/PARITY.md](docs/PARITY.md) | maintainers and Overwolf: the parity matrix and lab results |
| [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md) | Overwolf: open questions and interim behaviour |
| [docs/adr/](docs/adr/README.md) | maintainers: decision records |
| [docs/RELEASING.md](docs/RELEASING.md) | maintainers: how a release is made |
| [docs/MIGRATION.md](docs/MIGRATION.md) | ow-electron teams moving to Tauri |
| [docs/AD-FORMATS.md](docs/AD-FORMATS.md) | ad integrators: every format |
| [docs/api/](docs/api/README.md) | API reference overviews |
