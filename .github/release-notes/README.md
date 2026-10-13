# Release notes

The "Publish Ledger Release" workflow (`.github/workflows/prerelease.yml`) creates
a GitHub Release for each ledger tag it pushes. It uses the notes file in this
directory whose name matches the tag exactly.

## Convention

- One file per release, named `<tag>.md`, e.g. `ledger-9.1.0.0-rc.3.md`.
- The tag is `ledger-<epoch>.<wasm-version>` (e.g. `ledger-9.1.0.0-rc.3`), the
  same value the workflow tags and the npm package version.
- Copy `TEMPLATE.md`, fill it in, and commit it **before** running the release
  workflow.

## What the workflow does

- If a file `<tag>.md` exists here, the release body is taken from it.
- If it does not exist, the release is still created (so no release is ever
  missed) with GitHub auto-generated notes, and the run logs a warning.
- Tags containing `-rc`, `-alpha`, `-beta`, or `-performance` are marked as a
  pre-release.
- Re-running the workflow is safe: an existing release is left untouched.
