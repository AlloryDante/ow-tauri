# Lab runs

`e2e/run.mjs` checks the sample on macOS without ever showing a window or
taking the keyboard: that it starts and loads a test ad, that its restart
works, and it records the documentation stills. It is a development tool:
the normal page build never contains the driver, and the app has the lab
only with its `lab` Cargo feature, which is off by default and must never
ship.

```sh
# From examples/packages-sample, after `npm install` at the repository root.
node e2e/run.mjs                             # lab build, then one smoke run
node e2e/run.mjs --no-build                  # run the last lab build
node e2e/run.mjs --steps restart             # the restart check
node e2e/run.mjs --steps tour --theme light  # a still of every page
node e2e/run.mjs --page quickstart           # the quickstart window
node e2e/run.mjs --run-id my-run --timeout 240 --ad-wait 60000 --stall 60
```

| Option                | What it does                                                                                          |
| --------------------- | ----------------------------------------------------------------------------------------------------- |
| `--steps smoke`       | Default. Opens the Ads Tester, starts both slots, waits for `display_ad_loaded`, quits                |
| `--steps restart`     | The restart check (below)                                                                             |
| `--steps tour`        | A still of every page, the Ads Tester with both test ads, and the ad privacy settings window          |
| `--page quickstart`   | Page-host mode: [quickstart-vanilla](../../quickstart-vanilla)'s page and identity in this lab shell  |
| `--theme dark\|light` | The system appearance the app's pages see (`prefers-color-scheme`); default: the Mac's                |
| `--stills DIR`        | Where stills go (default `e2e/out/<run id>/stills`)                                                   |
| `--stall S`           | Kill a run whose driver records nothing for `S` seconds (default 60); its summary names the last step |
| `--timeout S`         | Kill a run after `S` seconds (default 300, tour 600)                                                  |

What a run does:

1. Builds the page with `vite build --mode lab` into `dist-lab/` (the only
   build that bundles [src/lab/driver.ts](../src/lab/driver.ts)), then a
   debug build with `--features lab` whose `TAURI_CONFIG` sets the lab
   bundle id `dev.ow-tauri.packages-sample.lab` and embeds `dist-lab/`.
   `CARGO_TARGET_DIR` is honoured (else `src-tauri/target/e2e`), and each
   page's binary is kept in `e2e/out/.bin/<page>/` for `--no-build`.
2. Starts a loopback sink and launches the app with `--test-ad` under an
   isolated `HOME`, with `OW_TAURI_LAB_INVISIBLE=1`. The app's analytics and
   consent experiment endpoints point at the sink (`OW_SAMPLE_LAB_SINK`), so
   nothing reaches Overwolf's servers except the test ad and consent pages
   themselves.
3. The invisible lab keeps the window on screen at alpha 0, click-through,
   never key, above other apps' windows (so none can cover it: WebKit
   stops the timers of a covered page), and the app never active
   (Accessory activation policy). The window monitor from
   `tools/parity-harness/lib/window-monitor.swift` watches every window of
   every process of the run; one visible window kills the run
   (`safety-kill`). A front check fails the run if any of them ever becomes
   the frontmost app.
4. The driver presses the sample's own buttons and records each step, a
   heartbeat every 5 s and each page visibility change through the lab
   commands into `e2e.jsonl`, then quits the app. It never clicks an ad.
   Stills are made in process (each `WKWebView` renders itself); nothing
   captures the screen.
5. The runner waits for the app and its WebKit processes to exit and writes
   `e2e/out/<run id>/summary.json`.

The restart check starts on no page in TEST, opens the CMP & settings page
(no ad guest, so no live ad loads) and presses "Restart with live ads",
then, in the new process, "Restart with test ads". It passes when three
processes reported, with the modes TEST, LIVE and TEST, the second and third
on the settings page (the restart keeps the page), and each old process had
exited within 10 s of the next one reporting.

A run passes (exit code 0) when `verdict` is `done`, `everVisible` and
`everFront` are `false`, its `check` holds (smoke: a test ad loaded; tour:
every still written; restart: the check above; quickstart: the still, with
its test ad loaded), nothing is `fatal` and `leftProcesses` is empty.
`e2e/out/` is ignored by git; its traces hold the lab machine's ids, so do
not publish them. The documentation images come from
`examples/ad-showcase/scripts/screenshots.mjs`, which runs the tour and the
quickstart page in both themes and checks the identity first.

`leftProcesses` counts WebKit processes that appeared after the launch under
the same responsible process (an app started from a terminal is not
responsible for its own WebKit processes). Another WebKit app started from
the same terminal during the run is counted too: run one lab app at a time.

Pure helpers of the runner are in [lab.mjs](lab.mjs) and tested by
[lab.test.mjs](lab.test.mjs) (`npm test`).
