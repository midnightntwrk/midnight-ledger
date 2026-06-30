# Releasing Midnight Ledger

This guide explains how to cut a release. The process is two GitHub Actions
workflows: **Prepare Ledger Release** (bumps versions, drafts notes, opens a PR)
and **Publish Ledger Release** (publishes npm + tags + a GitHub Release + Slack).

Releases are cut from a `ledger-N` branch (e.g. `ledger-8` for mainnet, `ledger-9`
for the next hardfork). Nothing is published automatically: publishing only
happens when someone manually runs the Publish workflow.

## TL;DR

1. **Prepare** -> Actions -> *Prepare Ledger Release* -> enter the new versions -> it opens a PR.
2. **Review** that PR: write the release notes, check the changelog, merge it.
3. **Rehearse** -> Actions -> *Publish Ledger Release* with `dry_run` checked.
4. **Publish** -> run it again with `dry_run` off; a maintainer approves and it ships.

## Before you start

- You need **write access** to the repo (manual workflow runs require it).
- Decide the **branch** you are releasing from (`ledger-8`, `ledger-9`, ...).
- One-time repo setup (already configured for maintainers): a protected
  `release` environment with required reviewers, and the
  `SLACK_TOPIC_LEDGER_WEBHOOK_URL` secret.

## Versioning

The git tag and GitHub Release are always `ledger-<display-version>`. The display
version is derived from the `ledger-wasm` crate version plus the epoch in
`flake.nix`:

| Branch    | `ledger-wasm` version | Tag / display version |
| --------- | --------------------- | --------------------- |
| ledger-8  | `8.1.1`               | `ledger-8.1.1`        |
| ledger-9  | `1.0.0-rc.4`          | `ledger-9.1.0.0-rc.4` |
| ledger-10 | `1.0.0`               | `ledger-10.1.0.0`     |

`ledger-8` keeps the epoch in the crate version (`8.x.y`); `ledger-9` and later
decouple it (the epoch lives in the package name and tag, the crate version
restarts at `1.x`). The workflow handles both automatically.

## Pre-releases (alpha / beta / rc)

A version with a pre-release suffix ships as a pre-release rather than a final
release. The usual progression toward a final version is:

```
-alpha.N  ->  -beta.N  ->  -rc.N  ->  (no suffix = final)
```

You cut a pre-release exactly like a normal release - just give the crate a
version with the suffix in Step 1. The suffix is carried straight through:

| Branch   | `ledger-wasm` version | Tag                      | npm dist-tag | GitHub Release |
| -------- | --------------------- | ------------------------ | ------------ | -------------- |
| ledger-8 | `8.2.0-rc.1`          | `ledger-8.2.0-rc.1`      | `rc`         | pre-release    |
| ledger-9 | `1.0.0-alpha.2`       | `ledger-9.1.0.0-alpha.2` | `alpha`      | pre-release    |
| ledger-9 | `1.0.0-rc.4`          | `ledger-9.1.0.0-rc.4`    | `rc`         | pre-release    |
| ledger-8 | `8.2.0`               | `ledger-8.2.0`           | `latest`     | Latest         |

What the suffix changes (everything else - artifacts, checksums, tests, tags -
is identical to a final release):

- **npm dist-tag:** the suffix becomes the npm tag (`rc`, `alpha`, `beta`, ...),
  so a plain `npm install @midnightntwrk/ledger-vN` (which resolves `latest`)
  keeps pointing at the last *final* release. Pre-releases are opt-in via
  `npm install @midnightntwrk/ledger-vN@rc`. Final versions (no suffix) get
  `latest`.
- **GitHub Release:** flagged as a pre-release, so it is not shown as "Latest".
  Auto-detected for `-rc`, `-alpha`, `-beta`, and `-performance`.
- **Slack:** the announcement says "pre-release" instead of "release".

## Step 1: Prepare the release

1. Actions -> **Prepare Ledger Release** -> *Run workflow*.
2. Select your branch (e.g. `ledger-8`).
3. Fill in **`versions`**: a comma-separated list of `crate=version` for every
   crate you are bumping. Only the listed crates change. Example for a `ledger-8`
   patch:

   ```
   ledger-wasm=8.1.1, ledger=8.1.1
   ```

   Add the others if they ship too, e.g.
   `onchain-runtime-wasm=3.1.1, onchain-runtime=3.1.1, zkir=2.1.1, zkir-wasm=2.1.1`.
4. Run it. The workflow then:
   - bumps each crate's `Cargo.toml` and refreshes `Cargo.lock`,
   - drafts a `## Ledger <version>` section in `CHANGELOG.md` from commit history,
   - scaffolds `.github/release-notes/ledger-<version>.md` from the template,
   - opens a `release/ledger-<version>` PR.
5. **Review the PR.** Edit `.github/release-notes/ledger-<version>.md` (write the
   real summary / breaking changes), tidy the changelog, confirm the versions,
   then merge it into the release branch.

> The release notes file is what becomes the GitHub Release body. See
> `.github/release-notes/README.md` for the convention.

## Step 2: Publish the release

Always rehearse with a dry run first.

1. Actions -> **Publish Ledger Release** -> *Run workflow* -> select the branch.
2. **Dry run:** check **`dry_run`** (leave `force` off). Run it. Every
   irreversible action becomes a logged no-op, but the build, tests, and artifact
   assembly all run (a dry run always builds, even if the version is already
   published). When it finishes, download the **`release-assets`** artifact from
   the run summary and confirm it contains the `.tgz` packages, test/coverage
   outputs, and `SHA256SUMS`. `docker-push` should show as *skipped*.
3. **Real run:** run it again with **`dry_run` unchecked**. A maintainer must
   approve the `release` environment before it proceeds.

### What a real publish does

- Publishes the `ledger`, `onchain-runtime`, and `zkir` wasm packages to
  `@midnightntwrk` on both GitHub Packages and npmjs (idempotent: an
  already-published version is skipped).
- Pushes the git tags `ledger-<version>`, `onchain-runtime-<version>`,
  `zkir-<version>`.
- Runs coverage, integration, and unit tests fresh on the release commit. These
  **must pass** or the GitHub Release is not created.
- Creates the **GitHub Release** `ledger-<version>` with your notes (pre-release
  auto-flagged), attaching the three `.tgz` packages, unit + integration test
  results, coverage reports, and `SHA256SUMS`.
- Posts an announcement to the **topic-ledger** Slack channel.

## The `force` flag

`force` is for **real runs only**: it deletes-then-republishes a version on
GitHub Packages before publishing (public npmjs is immutable and will error).
Use it only to overwrite a bad GitHub Packages version. You do **not** need it for
a dry run - a dry run always builds on its own.

## Troubleshooting

- **The `dry_run` / `force` inputs don't appear when I pick a branch.** The
  inputs are registered from the repo's default branch. They must exist there;
  this lands once the change is merged into the default branch.
- **"already published ... skipping".** The version already exists on the
  registry; bump the version (Step 1) or use `force` (GitHub Packages only).
- **The run takes a long time.** The fresh test + coverage + integration suites
  run on the release commit. Benchmarks are intentionally not part of the release
  (they run as the separate nightly workflow).
- **No Slack message.** Confirm the `SLACK_TOPIC_LEDGER_WEBHOOK_URL` secret is
  set; the step warns and skips if it is missing.
