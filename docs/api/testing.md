# `tauri-plugin-overwolf-api/testing`

A fake plugin for unit tests of app code that uses `tauri-plugin-overwolf-api`
(Vitest, or Jest with a DOM environment). It is built on
`@tauri-apps/api/mocks`. Import it from tests only. The source is
`packages/api/src/testing/index.ts`.

```ts
import { afterEach, expect, it } from 'vitest';
import { mockOverwolf, settle, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';

let overwolf: MockOverwolf;
afterEach(() => overwolf.restore());

it('mounts the ad', async () => {
  overwolf = mockOverwolf({ info: { testAd: true } });
  await import('tauri-plugin-overwolf-api/adview');
  const ad = document.createElement('owadview');
  document.body.append(ad);
  await settle();
  const [mount] = overwolf.mounts();
  overwolf.emit(mount.elementId, 'display_ad_loaded', {});
  expect(overwolf.callsOf('adview_mount')).toHaveLength(1);
});
```

## `mockOverwolf(options?): MockOverwolf`

Installs the fake for the current document. Every plugin command gets a
default answer, and every call is recorded.

| Option | Default | Meaning |
|---|---|---|
| `label` | `'main'` | the label of the current webview and window |
| `info` | `DEFAULT_INFO` | fields that replace those of `getInfo()` |
| `machineIds` | `{ muid: 'mock-muid-v2', muidV2: 'mock-muid-v2' }` | what `getMachineIds()` returns |
| `cmpRequired` | `false` | what `isCMPRequired()` returns |
| `commands` | none | command implementations by name, without the `plugin:overwolf\|` prefix. They replace the defaults. |

The defaults: `get_info` returns the info; the consent, switch, email-hash
and window-name commands resolve; `generate_user_email_hashes` returns fixed
fake hashes; `adview_mount` records the mount and returns a guest label
`owad-<n>`; `updater_check` returns `null` (no update). A command with no
default rejects with `unsupported` (`"<command> is not mocked"`); the
updater's download and install commands are such commands.

A `MockCommand` is `(args, original) => result`. It may return a promise or
throw. A thrown `{ code, message }` object rejects as the plugin does.
`original` is the default fake of the same command, so an override can wrap
it, for example to hold `adview_mount` open while a test sends events.

## `MockOverwolf`

| Member | What it does |
|---|---|
| `calls` | every plugin call in order, as `{ command, args }` |
| `callsOf(command)` | the arguments of every call of one command |
| `setCommand(command, implementation)` | replaces or adds a command implementation |
| `mounts()` | the mounted `<owadview>` elements in mount order, as `{ elementId, guestLabel, request }` |
| `emit(elementId, name, data?, source?)` | delivers an event on a mounted element's channel. `source` is `'guest'` (an ad page event, the default) or `'host'` (a lifecycle event such as `did-attach`). Throws for an unknown element id. |
| `restore()` | removes the mocks (`clearMocks()`) |

## `settle(rounds = 4): Promise<void>`

Waits until pending promise callbacks and zero-delay timers have run, so
the effects of a DOM change or a command reply are visible.

## `DEFAULT_INFO`

The `getInfo()` answer of the fake: a placeholder uid of 40 letters, phase
50, `testAd: true`, `adsSupported: true`, name `Test App`, version `1.0.0`,
and host label `tauri`.
