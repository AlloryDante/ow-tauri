# Overwolf onboarding

The steps from a new app to live Overwolf ads. Overwolf's own process for
ow-electron apps is the reference; this page says how each step works with
the Tauri plugin. Overwolf's documentation:
[dev.overwolf.com/ow-electron](https://dev.overwolf.com/ow-electron/getting-started/onboarding-resources/first-app).

Two points are not yet confirmed by Overwolf for apps built on Tauri:
enabling live ads in production, and uploading Tauri's installer in the
developer console. Talk to your Overwolf contact before you plan a launch
date. [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) tracks both.

## 1. Register the app

Apply to Overwolf and create the app in the developer console, as for an
ow-electron app. Overwolf identifies the app by its uid.

## 2. Get the uid

The plugin computes the uid exactly as ow-electron does: from `author` and
`name` in `plugins.overwolf`. `name` plays the part of ow-electron's
`productName`. See the uid with:

```sh
npm exec --no -- ow-tauri doctor
```

or `(await getInfo()).uid` in the app.

- If the console shows the same uid, pin `author` and `name` and never
  change them: a new value gives a new uid, and Overwolf sees a new app.
- If the console assigned a different uid, set it:

  ```json
  "plugins": { "overwolf": { "uid": "<uid from the console>" } }
  ```

A release build fails unless `uid`, or both `author` and `name`, are set.
An app that comes from ow-electron keeps its uid with `ow-tauri migrate`
([MIGRATION.md](MIGRATION.md#2-keep-the-uid-with-ow-tauri-migrate)).

## 3. Develop with test ads

Until Overwolf enables live ads, use test ads: `ads.testAd`,
`OW_TAURI_TEST_AD=1`, `--test-ad` or `Builder::test_ad(true)`
([GETTING-STARTED.md](GETTING-STARTED.md#6-run-with-test-ads)). Test ads
load through Overwolf's ad pages, as live ads do.

Follow Overwolf's ad guidelines for placement: the containers stay visible,
are not covered, and are not moved or made transparent while an ad shows.
[AD-FORMATS.md](AD-FORMATS.md) covers each format.

## 4. Add the privacy settings entry

Give users in consent regions a way to change their ad privacy choices.
Show an entry, for example in your settings page, when `isCMPRequired()` is
`true`, and open Overwolf's window from it:

```ts
import { isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';

if (await isCMPRequired()) {
  showPrivacyButton(() => openAdPrivacySettingsWindow());
}
```

The startup consent round runs by itself on every launch.

## 5. Submit for review

Build a release with test ads off and send it to Overwolf for review, as
Overwolf's process for ow-electron apps describes. Go through
[PRODUCTION-CHECKLIST.md](PRODUCTION-CHECKLIST.md) first.

## 6. Live ads

Overwolf enables live ads for the uid after the review passes. Nothing
changes in your code: a build without test ads requests live ads. Keep the
uid unchanged from here on.

## 7. Signing (optional)

Overwolf's build signing is off by default, and nothing in the plugin needs
it. If Overwolf asks for a signed build:

1. Turn it on: `"signing": { "enabled": true }` in `plugins.overwolf`.
2. Create a build key in the console. Put `OW_CLI_EMAIL`, `OW_CLI_API_KEY`
   and `OW_BUILD_KEY` in your CI secrets or in a git-ignored `.env`. Never
   commit them.
3. Run `npm exec --no -- ow-tauri sign --main dist/index.html` after the
   frontend build, and bundle the files it writes to `signed/`. The full
   configuration is in [MIGRATION.md](MIGRATION.md#10-sign-the-build-optional).
4. With `signing.owCertSigning`, Overwolf's certificate signs the app's exe
   through `ow-tauri sign-exe` as Tauri's `signCommand`.

`ow-tauri sign` stops when the uid Overwolf signed differs from the app's
uid, and tells you which `uid` to set. Overwolf's guide:
[App signing](https://dev.overwolf.com/ow-electron/guides/dev-tools/app-signing).

## 8. Updates

Overwolf serves an update feed per uid, for Windows installers. Build with
the plugin's `updater` feature and set `updater.publisherNames` (your
installer's certificate subject) or `updater.pubkey`
([api/js.md](api/js.md#updater), [CONFIG.md](CONFIG.md#updater)). Publish
the NSIS installer through the console; whether the console accepts Tauri's
installer is one of the open points above. On macOS and Linux, host
your own feed and use `tauri-plugin-updater`
([INTEROP.md](INTEROP.md#tauri-plugin-updater)).
