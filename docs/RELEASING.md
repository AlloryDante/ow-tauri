# Releasing

This is the maintainer runbook for publishing ow-tauri. Releases run only in
GitHub Actions, through [`release.yml`](../.github/workflows/release.yml).
Nobody publishes from a laptop.

Nothing is published yet; the project is on GitHub only. Until the first
registry release, run the workflow with `dry_run=true` only, and push no
tags.

## What is published

| Package | Registry | Notes |
|---|---|---|
| `tauri-plugin-overwolf-unstable` | crates.io | Published first: the plugin depends on it. |
| `tauri-plugin-overwolf` | crates.io | The plugin. docs.rs builds its documentation after the publish. |
| `tauri-plugin-overwolf-api` | npm | Browser-only JavaScript API. |
| `tauri-plugin-overwolf-cli` | npm | The `ow-tauri` command. |

Not published: `packages/guest-shims` (private; its build output ships inside
the plugin crate as `js/`), the ACL test app, the examples and `tools/`.

All four packages share one version. A prerelease (`1.0.0-rc.1`) goes to the
npm dist-tag `next` and becomes a GitHub prerelease. A stable version
(`1.0.0`) goes to `latest`.

## The release scripts

All three use Node.js built-ins only. CI runs their tests.

| Command | What it does |
|---|---|
| `node scripts/release/version-sync.mjs check` | Lists every place that carries the version and fails if they differ. |
| `node scripts/release/version-sync.mjs set <version>` | Writes the version everywhere: the Cargo manifests, the plugin's requirement on the helper crate, the npm packages, the example dependency ranges, `RUNTIME_VERSION` in the API package, `package-lock.json` and every `Cargo.lock`. |
| `node scripts/release/changelog.mjs stamp <version>` | Replaces `Unreleased` in the version's CHANGELOG heading with today's date. |
| `node scripts/release/changelog.mjs section <version>` | Prints the version's CHANGELOG section (the GitHub release notes). |
| `node scripts/release/identity-check.mjs` | Fails if a tracked file contains a denied term (see the script's header). |
| `node --test "scripts/release/*.test.mjs"` | The scripts' tests. |

## One-time setup (owner)

Do this once, before the first release.

1. **crates.io.** Sign in at https://crates.io with the GitHub account that
   will own the crates. That account must use two-factor authentication.
   Verify your email address in the account settings. Then create an API
   token (Account Settings → API Tokens):
   - scopes `publish-new` and `publish-update`;
   - crates `tauri-plugin-overwolf*`;
   - a short expiry, for example 30 days.
2. **npm.** Create or use an npm account with two-factor authentication on.
   Create a granular access token (https://docs.npmjs.com/creating-and-viewing-access-tokens)
   with read and write access to packages and a short expiry. The packages do
   not exist yet, so the token cannot be limited to them before the first
   release. The workflow cannot type a one-time password: if npm asks whether
   the token may bypass two-factor authentication, allow it.
3. **GitHub environment.** In the repository, open Settings → Environments →
   New environment and name it `release`. Then:
   - Required reviewers: add yourself. If you start releases yourself, leave
     "Prevent self-review" off.
   - Deployment branches and tags: Selected branches, add `main`.
   - Environment secrets: add `CARGO_REGISTRY_TOKEN` (the crates.io token) and
     `NPM_TOKEN` (the npm token). Put them in the environment, not in the
     repository secrets, so only the approved publish jobs can read them.
4. **Public repository.** npm provenance needs a public GitHub repository that
   matches the packages' `repository` field.

## Before a release

- `main` CI is green.
- The parity check against ow-electron ([PARITY.md](PARITY.md)) passed on the
  commit you release.
- The version's section in [CHANGELOG.md](../CHANGELOG.md) is complete and
  honest, with its known limits. Its heading reads
  `## [<version>] - Unreleased` until the bump below.
- First release only: the install docs use the GitHub forms (a `git`
  dependency and `npm pack` tarballs) and say "not on crates.io or npm yet".
  Switch them to the registry forms in the release commit. `git grep -n -i
  -e "tgz" -e "not on crates.io" -e "not on npm" -e "git = \"https"` finds
  them. Point the
  CHANGELOG links at the tag (`compare/v<version>...HEAD` for
  `[Unreleased]`, `releases/tag/v<version>` for the version).

## Make a release

The commands use `1.0.0-rc.1`; replace it with your version.

1. **Rehearse (optional, any time).** This runs every check on the current
   `main` and bumps the version inside the runner only:

   ```sh
   gh workflow run release.yml --ref main -f version=1.0.0-rc.1 -f dry_run=true
   gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
   ```

   The run warns "Rehearsal" while the manifests carry another version.

2. **Bump the version on `main`.**

   ```sh
   node scripts/release/version-sync.mjs set 1.0.0-rc.1
   node scripts/release/changelog.mjs stamp 1.0.0-rc.1
   git diff --stat
   git commit -am "chore(release): 1.0.0-rc.1"
   git push origin main
   ```

   Wait until CI is green on that commit.

3. **Dry run on the bumped commit.** Same command as step 1. This time there
   is no rehearsal warning, and the "CI passed on this commit" step must say
   so. The run summary shows the release notes and the SHA-256 of every file.

4. **Publish.**

   ```sh
   gh workflow run release.yml --ref main -f version=1.0.0-rc.1 -f dry_run=false
   ```

   The run releases the head of `main`, so do not push other commits between
   the bump and the release. The `verify` job runs the same checks and
   refuses the run when the manifests do not carry the version, the CHANGELOG
   heading has no date, or CI did not pass on the commit (it waits up to 45
   minutes for a CI run that is still going; a CI run cancelled by a later
   push does not count). Then:
   - **Publish crates** waits for your approval (the run page shows "Review
     deployments"). It publishes `tauri-plugin-overwolf-unstable`, waits until
     the crates.io index serves it, then publishes `tauri-plugin-overwolf`.
   - **Publish npm packages** waits for a second approval. It publishes the
     tarballs the `verify` job packed, with `--provenance --access public`
     and the dist-tag `next` (prerelease) or `latest`.
   - **Tag and GitHub release** runs after both. It creates the tag
     `v1.0.0-rc.1` on the released commit and a GitHub release (a prerelease
     for an rc) with the CHANGELOG section, the four files and `SHA256SUMS`.

5. **Check the registries.**

   ```sh
   cargo info tauri-plugin-overwolf@1.0.0-rc.1
   cargo info tauri-plugin-overwolf-unstable@1.0.0-rc.1
   npm view tauri-plugin-overwolf-api dist-tags
   npm view tauri-plugin-overwolf-cli dist-tags
   ```

   On npmjs.com, each package page shows a provenance section that links the
   tarball to the workflow run. In a test project, `npm audit signatures`
   verifies it. docs.rs shows the plugin's documentation a few minutes after
   the publish.

## When a run fails

- Each publish step skips a version that is already on its registry, so you
  can re-run the failed jobs of the same run.
- A published version can never be replaced. To withdraw one, yank the
  crates (`cargo yank --version 1.0.0-rc.1 tauri-plugin-overwolf`) and
  deprecate the npm packages (`npm deprecate tauri-plugin-overwolf-api@1.0.0-rc.1 "<reason>"`),
  then fix forward with the next version (`1.0.0-rc.2`).

## After the first release: trusted publishing

Long-lived tokens are only for the first publish: crates.io and npm can only
trust a workflow for a package that already exists. After the first release:

1. **crates.io.** For each of the two crates, open its Settings → Trusted
   Publishing and add a GitHub publisher: repository `AlloryDante/ow-tauri`,
   workflow `release.yml`, environment `release`.
2. **npm.** For each of the two packages, open its Settings → Trusted
   publishing and add GitHub Actions with the same repository, workflow file
   `release.yml` and environment `release`.
3. **Change `release.yml`.**
   - `publish-crates`: add `id-token: write`, get a short-lived token with
     `rust-lang/crates-io-auth-action` (pinned by SHA) and pass its output as
     `CARGO_REGISTRY_TOKEN`.
   - `publish-npm`: npm trusted publishing needs npm 11.5.1 or later (Node.js
     24 ships it); drop `NODE_AUTH_TOKEN`.
   - Run a dry run, then release the next version with the new setup.
4. **Delete the tokens** on crates.io and npm, and delete the
   `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN` environment secrets.

## From release candidate to 1.0.0

Ship `1.0.0-rc.N` releases while integrators try them, for at least two
weeks. Then release `1.0.0` the same way after a fresh parity check. Its
CHANGELOG section lists the changes since the last rc.
