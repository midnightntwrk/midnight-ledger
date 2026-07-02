# Releasing Midnight Ledger

This guide explains how to cut a release. The process is two GitHub Actions
workflows: **Prepare Ledger Release** (bumps versions, drafts notes, opens a PR)
and **Publish Ledger Release** (publishes npm + crates.io + tags + a GitHub
Release + Slack).

Releases are cut from a `ledger-N` branch (e.g. `ledger-8` for mainnet, `ledger-9`
for the next hardfork). Nothing is published automatically: publishing only
happens when someone manually runs the Publish workflow.

## TL;DR

1. **Prepare** -> Actions -> *Prepare Ledger Release* -> enter the new versions -> it opens a PR.
2. **Review** that PR: write the release notes, check the changelog, merge it.
3. **Rehearse** -> Actions -> *Publish Ledger Release* with `dry_run` checked.
4. **Publish** -> run it again with `dry_run` off; a maintainer approves and it
   ships npm + tags + the GitHub Release, and (for a *final* release) pushes the
   crates to crates.io.

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
- **crates.io:** the Publish workflow skips crates.io for pre-releases. They are
  consumed via a cargo patch against the git tag instead, which needs the manual
  crate-isolation step described in [crates.io](#step-3-cratesio-final-releases-only).

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
   - bumps each crate's version (`Cargo.toml`, or the root `[workspace]
     package.version` for `ledger`/`zswap`/`proof-server`) and refreshes `Cargo.lock`,
   - pins internal dependency requirements on bumped crates to the new exact
     version (needed so pre-releases resolve),
   - drafts a `## Ledger <version>` section in `CHANGELOG.md`, and a
     `## Version <version>` section in each bumped crate's `CHANGELOG_<crate>.md`,
     from commit history,
   - regenerates the wasm markdown API docs under `docs/api/**`,
   - scaffolds `.github/release-notes/ledger-<version>.md` from the template,
   - opens a `release/ledger-<version>` PR with all of the above in one signed commit.
5. **Review the PR.** The changelog and doc changes are auto-generated *drafts*:
   edit `.github/release-notes/ledger-<version>.md` (real summary / breaking
   changes), tidy the top-level and per-crate changelogs, skim the `docs/api/**`
   diff, confirm the versions, then merge it into the release branch.

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
- For a **final** release, publishes the workspace crates to **crates.io** once
  the test suite passes (skipped for pre-releases - see [Step 3](#step-3-cratesio-final-releases-only)).
- Posts an announcement to the **topic-ledger** Slack channel.

## The `force` flag

`force` is for **real runs only**: it deletes-then-republishes a version on
GitHub Packages before publishing (public npmjs is immutable and will error).
Use it only to overwrite a bad GitHub Packages version. You do **not** need it for
a dry run - a dry run always builds on its own.

## Step 3: crates.io (final releases only)

For a **final** release (no pre-release suffix) the Publish workflow also pushes
the crates to crates.io - this is the `crates-io` job, which runs after the build
and the full test suite are green (crates.io is immutable, so it publishes only
once everything passes, unlike npm which goes out earlier). The job:

- is **skipped for pre-releases** (their display version contains a `-`); see below,
- publishes every workspace crate that is not `publish = false`, in dependency
  order, via `cargo workspaces publish` (which waits for the index to propagate
  between crates and skips any version already on crates.io, so re-runs are safe),
- runs on dry runs too, as a `--dry-run` rehearsal that packages every crate but
  uploads nothing.

It needs the `CARGO_REGISTRY_TOKEN` secret (a crates.io API token with publish
rights). There is no `force`: crates.io is immutable, so a bad version needs a
version bump, not an overwrite.

> Possible gotcha when bumping versions in Step 1: `ledger/Cargo.toml`'s `zswap`
> dependency version must match `zswap/Cargo.toml`. The same holds for other
> internal dependencies when their update is a breaking change.

### Pre-releases are not pushed to crates.io

Most of our crates are pulled in as transitive dependencies, and cargo's resolver
rejects any non-direct reference to a pre-release version. To avoid massive
version churn we rely on cargo **patches** against a git tag instead. A crate
`foo` pre-released at `1.2.3-rc.1`:

- has a declared crate version of `1.2.3`, so the patch resolves for `^1.0.0` ranges,
- is released as a *tag* `foo-1.2.3-rc.1`, pulled in as a cargo patch,
- is **not** pushed to crates.io (such a release is practically unused and only
  causes confusion).

Consumers patch it like this:

```toml
[dependencies]
foo = "^1.0.0"

[patch.crates-io]
midnight-foo = { git = "https://github.com/midnightntwrk/midnight-ledger", tag = "foo-1.2.3-rc.1" }
```

Special cases (may be revisited):

- `zswap` and `ledger` *do* carry the pre-release suffix in their version, so
  consumers must specify their version *exactly*.
- `proof-server` and the `*-wasm` crates also carry the suffix but are assumed
  not to be imported directly (they are not released on crates.io).
- `ledger` has a crate-override tag `crate-ledger-1.2.3-rc.1`, because
  `ledger-1.2.3-rc.1` is already reserved for the full (non-isolated) repo state.

### Isolated crate tags

These crate tags must be specially crafted: cargo's `git` resolution prefers the
local dependency spec over the crates.io one, which can duplicate dependencies.
For example, if a consumer pulls in both `foo` and `bar`, patches them to
`foo-1.2.3-rc.1` and `bar-2.3.4-rc.1`, and `foo` depends on `bar`, you end up
with two incompatible instances of `bar` (one inside the `foo` tag, one from the
`bar` tag). To prevent this, *isolate* the crate in its pre-release tag. When
releasing `foo`:

- in the root `Cargo.toml`, comment out every crate other than `foo`,
- in `foo/Cargo.toml`, remove the `path = "..."` entry from each `dependencies`
  and `dev-dependencies` entry (adding a `version = "..."` if necessary).

This forces `foo-1.2.3-rc.1`'s dependency to be `bar = "^2.0.0"`, which is then
also patched to `bar-2.3.4-rc.1`. Note the isolated tag *may not build by itself*
if the pre-releases depend on each other - that is fine.

## Backporting and chained releases

As a first preference a release is taken from the relevant `ledger-*` branch.
Sometimes changes need to land in a prior release while pulling in minimal noise,
for instance:

- security fixes for prior versions no longer under active development,
- additions to a release candidate, or promoting an rc to a full release.

In those cases base the release on *the prior release*: a security fix `7.0.1` is
based on the `ledger-7.0.0` tag; a pre-release `8.1.0-rc.2` is based on the prior
`ledger-8.1.0-rc.1`. Cherry-pick the necessary changes onto that basis, then
follow the standard process, still opening a PR into the relevant branch.

This will likely require resolving a versioning conflict on the release PR *in
favour of the target branch*. For example a `ledger-8.1.0-rc.2` may conflict
because it sets the version to `-rc.2` while the target branch is on `ledger-8.2.0`;
the target branch wins, but only after the `ledger-8.1.0-rc.2` tag has been cut.

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
