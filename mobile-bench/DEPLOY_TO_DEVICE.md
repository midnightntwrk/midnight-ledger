# Deploying mobile-bench to a physical Android device

The `mobile-bench` workspace produces two artifacts that run on Android, both
arm64-v8a — same binaries work on the Pixel Fold emulator and on a Samsung
S24 Ultra (or any other arm64 phone) without rebuild:

1. **APK with Dioxus UI** (`io.iohk.midnight.bench`) — visual benchmark
   with a button to trigger a proof and a panel showing prove/verify time.
2. **`bench-runner` ELF binary** — headless CLI; logs JSON-style timings
   to stdout. Faster to iterate on, no UI overhead.

This guide covers both paths on a real phone.

## 1. Phone prerequisites (one-time)

On the Samsung S24 Ultra (or other Android 11+ device):

1. **Settings → About phone → Software information** → tap *Build number* 7×
   to unlock Developer Options.
2. **Settings → Developer options** → enable:
   - *USB debugging*
   - *Stay awake* (helpful while benching)
3. Plug in via USB-C. Choose **File transfer (MTP)** mode if prompted. A
   "Allow USB debugging?" dialog appears the first time — tick *Always
   allow from this computer* and accept.

Verify the host sees the device:

```bash
export ANDROID_HOME=$HOME/Library/Android/sdk
export PATH=$ANDROID_HOME/platform-tools:$PATH

adb devices
# Expect:
#   List of devices attached
#   R5CW123ABCD     device          ← phone serial
#   emulator-5554   device          ← (only if emulator also running)
```

If multiple devices are attached, target the phone explicitly with
`adb -s <serial> ...` for every command below. If only the phone is
attached, plain `adb` works.

## 2. Path A — APK (Dioxus UI)

The APK ships an arm64-v8a `libdioxusmain.so` that contains the full
proving stack. Build once on the host, install on the phone, launch from
the app drawer.

### Build the APK

```bash
cd mobile-bench/dioxus-bench

# (a) Cross-compile the Rust cdylib for arm64-v8a → drops the .so into
#     android/app/src/main/jniLibs/arm64-v8a/.
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs \
  build --release -p dioxus-bench --lib

# (b) Wrap into a debug APK (signed with the default debug keystore).
cd android
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home \
  ./gradlew assembleDebug
# APK ends up at: app/build/outputs/apk/debug/app-debug.apk
```

For a release-signed APK (smaller, ProGuard-optimised), use
`./gradlew assembleRelease` instead — but you'll need to wire up your own
keystore in `app/build.gradle.kts`.

### Push parameter cache (one-time per device)

The proving stack needs the BLS-12-381 SRS files. Without network access
on the device they must be pre-pushed to a writable location the app can
read. The path defaults to `/data/local/tmp/midnight-pp/` (see
[dioxus-bench/src/platform/android.rs](dioxus-bench/src/platform/android.rs)).

```bash
PARAMS=$HOME/.cache/midnight/zk-params
adb shell mkdir -p /data/local/tmp/midnight-pp
for f in "$PARAMS"/bls_midnight_2p{4,5,6,7,8,9,10,11}; do
  [ -f "$f" ] && adb push "$f" /data/local/tmp/midnight-pp/
done
```

If you don't have the params locally, fetch them once from
`srs.midnight.network` over wifi and rerun the loop above.

### Install + launch

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n io.iohk.midnight.bench/dev.dioxus.main.MainActivity
```

Or open *Midnight Proof Bench* from the phone's app drawer. Tap **Run zkir
example** — the prove/verify timings appear in the *Last run* panel and
also stream to logcat:

```bash
adb logcat -v brief RustStdoutStderr:V '*:S'
```

### Uninstall

```bash
adb uninstall io.iohk.midnight.bench
```

## 3. Path B — `bench-runner` (headless CLI)

Faster to iterate; no UI. Cross-compile, push, run.

```bash
# Build
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a build -p prover-core --bin bench-runner --release

# Push runner + params (params step skippable if already pushed for path A)
adb push target/aarch64-linux-android/release/bench-runner /data/local/tmp/
adb shell chmod 755 /data/local/tmp/bench-runner

# Run
adb shell 'MIDNIGHT_PP=/data/local/tmp/midnight-pp \
  BENCH_CACHE_DIR=/data/local/tmp/bench-cache \
  /data/local/tmp/bench-runner'
```

Output is a JSON object with `prove_ms`, `verify_ms`, `proof_bytes`, and
`verified`. Pipe to `jq` for parsing.

## 4. Capturing real-device numbers for [RESULTS.md](RESULTS.md)

Run each path 3× with a warm cache and record median. The first run pays
keygen cost (~+50ms); discard it.

```bash
# CLI, 4 runs (1 warm-up + 3 measurements)
for i in 1 2 3 4; do
  adb shell 'MIDNIGHT_PP=/data/local/tmp/midnight-pp \
    BENCH_CACHE_DIR=/data/local/tmp/bench-cache \
    /data/local/tmp/bench-runner'
done
```

Add a row to the *Latency snapshot* table in
[RESULTS.md](RESULTS.md) under `Samsung S24 Ultra`.

## 5. Troubleshooting

- **`adb devices` shows `unauthorized`** — accept the trust prompt on the
  phone screen. If missed, `adb kill-server && adb start-server` and
  re-plug the cable.
- **APK install fails with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`** — a
  previous version is installed with a different signature; uninstall
  first (`adb uninstall io.iohk.midnight.bench`).
- **Splash screen hangs forever** — `libdioxusmain.so` was built with both
  `dioxus/desktop` and `dioxus/mobile` features active; rebuild after
  confirming
  [dioxus-bench/Cargo.toml](dioxus-bench/Cargo.toml) gates the `desktop`
  dep behind `cfg(not(target_os = "android"))`.
- **`bench-runner` exits with `Permission denied`** — re-`chmod 755` the
  pushed binary; some devices reset exec bits on push.
- **Proving fails with "params not found"** — the `MIDNIGHT_PP` directory
  is empty or missing `bls_midnight_2pN` files for the circuit's `k`.
  Re-push step 2's loop.
