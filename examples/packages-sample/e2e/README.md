# Lab smoke run

`e2e/run.mjs` checks on macOS that the sample starts and loads a test ad,
without ever showing a window or taking the keyboard. It is a development
tool: the normal page build never contains the driver, and the app has the
lab only with its `lab` Cargo feature, which is off by default and must
never ship.

```sh
# From examples/packages-sample, after `npm install` at the repository root.
node e2e/run.mjs                    # lab build, then one smoke run
node e2e/run.mjs --no-build         # run the last lab build
node e2e/run.mjs --run-id my-run --timeout 240 --ad-wait 60000
```

What a run does:

1. Builds the page with `vite build --mode lab` into `dist-lab/` (the only
   build that bundles [src/lab/driver.ts](../src/lab/driver.ts)), then a
   debug build with `--features lab` whose `TAURI_CONFIG` sets the lab
   bundle id `dev.ow-tauri.packages-sample.lab` and embeds `dist-lab/`.
   `CARGO_TARGET_DIR` is honoured.
2. Starts a loopback sink and launches the app with `--test-ad` under an
   isolated `HOME`, with `OW_TAURI_LAB_INVISIBLE=1`. The app's analytics and
   consent experiment endpoints point at the sink (`OW_SAMPLE_LAB_SINK`), so
   nothing reaches Overwolf's servers except the test ad pages themselves.
3. The invisible lab keeps the window on screen at alpha 0, click-through,
   never key, and the app never active (Accessory activation policy). The
   window monitor from `tools/parity-harness/lib/window-monitor.swift`
   watches every window of the app; one visible window kills the run
   (`safety-kill`). A front check fails the run if the app ever becomes
   the frontmost app.
4. The driver opens the Ads Tester, presses the sample's own "Start ad"
   buttons on both slots, waits for `display_ad_loaded`, records each step
   through the lab commands into `e2e.jsonl`, and quits the app. It never
   clicks an ad.
5. The runner waits for the app and its WebKit processes to exit and writes
   `e2e/out/<run id>/summary.json`.

A run passes (exit code 0) when `verdict` is `done`, `everVisible` and
`everFront` are `false`, `displayAdLoaded` is `true`, nothing is `fatal`
and `leftProcesses` is empty. `e2e/out/` is ignored by git; its traces hold
the lab machine's ids, so do not publish them.

`leftProcesses` counts WebKit processes that appeared after the launch under
the same responsible process (an app started from a terminal is not
responsible for its own WebKit processes). Another WebKit app started from
the same terminal during the run is counted too: run one lab app at a time.

Pure helpers of the runner are in [lab.mjs](lab.mjs) and tested by
[lab.test.mjs](lab.test.mjs) (`npm test`).
