# Documentation

Every page in `docs/`, plus the CLI reference and the example READMEs,
grouped by what you are doing. Each line says the question the page
answers. If you are new, start with
[GETTING-STARTED.md](GETTING-STARTED.md); if you come from ow-electron,
start with [MIGRATION.md](MIGRATION.md).

## Build with it

| Page | Answers |
|---|---|
| [GETTING-STARTED.md](GETTING-STARTED.md) | How do I take a new Tauri 2 app to its first Overwolf test ad? |
| [MIGRATION.md](MIGRATION.md) | How do I move an ow-electron app to Tauri and keep its uid? |
| [CONFIG.md](CONFIG.md) | What does each `plugins.overwolf` key in `tauri.conf.json` do? |
| [AD-FORMATS.md](AD-FORMATS.md) | How do I show each ad format, and which events should I expect? |
| [INTEROP.md](INTEROP.md) | In what order and with which settings do I use the plugin next to Tauri's official plugins? |
| [COMPATIBILITY.md](COMPATIBILITY.md) | Which Tauri, Rust, Node.js and OS versions does the plugin support? |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | An ad does not fill or a command fails: what is wrong and how do I fix it? |

### API reference

| Page | Answers |
|---|---|
| [api/README.md](api/README.md) | Which reference page covers the thing I am calling? |
| [api/js.md](api/js.md) | What does each function of `tauri-plugin-overwolf-api` do? |
| [api/owadview.md](api/owadview.md) | Which attributes, members and events does `<owadview>` have? |
| [api/rust.md](api/rust.md) | How do I register and configure the plugin in Rust, and what does the crate export? |
| [api/permissions.md](api/permissions.md) | Which permission set do I grant to which webview? |
| [api/testing.md](api/testing.md) | How do I unit-test app code that uses the API without a running app? |
| [packages/cli/README.md](../packages/cli/README.md) | Which commands and options does `ow-tauri` have? |

### Examples

| Example | Answers |
|---|---|
| [quickstart-vanilla](../examples/quickstart-vanilla/README.md) | What does the GETTING-STARTED app look like when it is done? |
| [quickstart-react](../examples/quickstart-react/README.md) | What does the same app look like in React 19? |
| [ad-showcase](../examples/ad-showcase/README.md) | How does every ad format behave, and how do I present the demo? |
| [packages-sample](../examples/packages-sample/README.md) | What does Overwolf's ow-electron sample look like on Tauri? |

## Ship it

| Page | Answers |
|---|---|
| [PRODUCTION-CHECKLIST.md](PRODUCTION-CHECKLIST.md) | What must I check before I release my app? |
| [OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md) | How do I go from test ads to live ads, and how do signing and updates work with Overwolf? |
| [SECURITY.md](SECURITY.md) | What does the plugin defend against, what does it write to disk, and what must my app do itself? |

## Understand how it works

| Page | Answers |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | How does the plugin host Overwolf's ad and consent pages inside a Tauri app? |
| [adr/](adr/README.md) | Why was each design decision made, and which records are superseded? |

## Review for Overwolf

| Page | Answers |
|---|---|
| [CONTRACT.md](CONTRACT.md) | What exactly does Overwolf receive from an app on the plugin? |
| [PARITY.md](PARITY.md) | How is the plugin compared with ow-electron, where does each behaviour stand, and what differs? |
| [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) | Which questions about Overwolf's services are settled, and what does the plugin do in the meantime? |

The root README has a [section for the Overwolf team](../README.md#for-the-overwolf-team)
with a reading order and how to rerun the proof.

## Work on the plugin

| Page | Answers |
|---|---|
| [CONTRIBUTING.md](../CONTRIBUTING.md) | How do I set up the repository, and which checks must a change pass? |
| [RELEASING.md](RELEASING.md) | How does a maintainer publish a release? (Nothing is published yet.) |
| [tools/parity-harness/README.md](../tools/parity-harness/README.md) | How do I record what ow-electron does and compare it with the plugin? |
| [SECURITY.md](../SECURITY.md) (repository root) | Which versions get security fixes, and how do I report a vulnerability? |
