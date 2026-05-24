# `react-native-demo/scripts/`

Tooling for iterating on the RN host integration without
round-tripping through screenshots / manual taps.

## `midnight-sim` — programmatic iOS Simulator driver

A bash CLI that wraps every step of the build + install +
launch + diagnose loop. Designed for running headless: every
subcommand reports its outcome in one line so you can chain
them or grep them.

### Subcommands

```
midnight-sim status        — 1-line health (sim/metro/installed/running)
midnight-sim metro         — start Metro detached if not running
midnight-sim metro-kill    — stop Metro
midnight-sim build         — re-link prover + pod install + xcodebuild
midnight-sim install       — simctl install the .app
midnight-sim launch        — terminate + launch with bundle URL pre-set
midnight-sim reload        — Cmd-R to the focused Simulator window
midnight-sim logs [N]      — tail last N lines of /tmp/metro.log (default 50)
midnight-sim logs-watch [s]— tail Metro for s seconds, filter errors / useBench
midnight-sim screen [path] — screenshot + sample pixels; report RED-ERROR / DARK-UI / UNKNOWN
midnight-sim cycle         — full pipeline: build → install → launch → watch logs 30s → screenshot
```

### Environment knobs (defaults shown)

```bash
MIDNIGHT_RN_HOST=/tmp/midnight-rn-host/MidnightDemoApp
MIDNIGHT_SIM_ID=76B99C81-BE72-4A93-A443-7F244723AAF3   # iPhone 17 Pro arm64
MIDNIGHT_BUNDLE_ID=org.reactjs.native.example.MidnightDemoApp
MIDNIGHT_METRO_LOG=/tmp/metro.log
MIDNIGHT_SCREEN_OUT=/tmp/midnight-sim-screen.png
```

### What it solved

The CLI exists because chasing a stable RN host build by
manually round-tripping screenshots was burning hours. With
this:

- **status** tells me in one line whether the simulator is
  booted, Metro is serving, the app is installed, and the app
  is running — no screenshot needed.
- **build** captures the build log and prints only the last 5
  `error:` lines if anything fails; full log lives at
  `/tmp/midnight-build.log` for deeper diagnosis.
- **screen** captures + classifies the top-of-screen pixel as
  `RED-ERROR-OVERLAY` (Hermes red), `DARK-UI` (the app's dark
  theme is rendering), or `UNKNOWN`. Doesn't OCR but the color
  classification is enough to know what state the app's in.
- **logs-watch** filters Metro stdout for the diagnostic
  patterns I added in the demo's `App.tsx` global error
  handler (`[GLOBAL-ERR]`) and the `useBench` traces.
- **cycle** chains everything for an unattended rebuild +
  validate. Single command from clean state to "is the app
  showing or red-screening?"

### Known issues surfaced via the CLI

These were caught by running `cycle` and reading the output,
not by screenshot:

| What I learned | Where it surfaced |
|---|---|
| ubrn CLI v0.31.0-2 from latest git emits C++ symbols the npm runtime doesn't expose | `build` failed with `error: no member named 'arraybufferToUint8Array' in namespace 'uniffi_jsi'` |
| Fix: pin CLI to `cargo install --git ... --rev b7c8a4e` (the commit matching the npm publish) | After re-install, `build` passed the C++ stage |
| ubrn-generated `.h` references `RNNativeModuleSpec.h` which RN codegen produces only when `codegenConfig.name == "RNNativeModuleSpec"` in package.json | `build` failed with `fatal error: 'RNNativeModuleSpec.h' file not found` |
| RN 0.74 needs MORE than `RCT_NEW_ARCH_ENABLED=1` env var to actually enable new arch — `AppDelegate.mm` and the Xcode project's Codegen config need separate opt-in | `logs-watch` showed `Bridgeless mode: false. TurboModule interop: false` even after the build succeeded |
| `react-native-screens` < 3.35 doesn't compile with RN 0.74 new arch | Dropped from the demo's deps entirely; plain `useState` tab switching now |

### Sample run

```
$ midnight-sim cycle
[midnight-sim] ─── full cycle ───
[midnight-sim] Metro already running on :8081
[midnight-sim] boot sim (if needed)
[midnight-sim] re-link prover package
[midnight-sim] pod install (with RCT_NEW_ARCH_ENABLED=1)
[midnight-sim] xcodebuild Debug (this can take 5-10 min from cold)
[midnight-sim] build OK — /tmp/.../MidnightDemoApp.app
[midnight-sim] installed
[midnight-sim] launched (pid 27049)
[midnight-sim] sleeping 10s for bundle download + JS execution
[midnight-sim] screen 1206x2622 DARK-UI (...) → /tmp/midnight-sim-screen.png
[midnight-sim] watching Metro for 20s; filtering errors / useBench traces
... [filtered Metro output] ...
[midnight-sim] ─── cycle done ───
[midnight-sim] sim=booted  metro=up  app_installed=yes  app_running=1
```

## See also

- `react-native-prover/README.md` — the package's own README,
  including the gated-on-environment-alignment caveats
- Architecture doc §14 — full RN packaging decision history
