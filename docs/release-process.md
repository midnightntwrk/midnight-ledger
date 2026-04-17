# Release process

The midnight ledger release process is fairly involved, partly due to it having
multiple different outputs, and partly because of it consisting of multiple
crates that evolve independently. The latter is because different major
versions of the ledger share some of their low-level infrastructure, especially
the `storage-core` crate.

## General release steps

The release steps are generally as follows. Each release should be done in its own PR:

1. Ensure all versions (`Cargo.toml` & `package.json`) have been updated *if*
   the relevant code has changes. Cosmetic (that is, non-functional changes)
   should not be released.
   Possible gotcha: `ledger/Cargo.toml`'s `zswap` dependency version needs to
   be updated to match `zswap/Cargo.toml`. This is also true for other internal
   dependencies if their update is a breaking change.
2. Ensure that `CHANGELOG*.md` are up-to-date.
3. Regenerate the wasm documentation by, for each of `ledger-wasm`, `zkir-wasm`, and `onchain-runtime-wasm`:
   1. Entering the respective directory.
   2. Running `npm install && npm run build:markdown-docs`
   3. Commit this step separately – it makes a lot of noise!
4. If this is a pre-release, do the steps in [#pre-release-considerations](Pre-release considerations below).
5. Once satisfied, push and wait for regular CI to run through, as well as getting a PR review.
6. Run the release workflow against the PR branch.
7. Promote the resulting wasm & docker artifacts to public.
8. If this is *not* a pre-release, push the relevant crates to crates.io.

## Pre-release considerations

Pre-releases should generally *not* be pushed to crates.io. Further, because most of our dependencies are transient dependencies, managing pre-releases in cargo is tricky, as crate resolution will reject any non-direct reference to this version. To avoid massive amount of version churn, we rely on cargo patches to resolve such pre-releases instead.

In practice, this means two things: a crate `foo` pre-released at `1.2.3-rc.1` will:
- Have a declared crate version of `1.2.3`, so that the patch will resolve for `^1.0.0` ranges.
- Be released as a *tag* `foo-1.2.3-rc.1`, which can then be pulled in as a cargo patch.
- *Not* be pushed to crates.io, as such a release practically won't be used, and will cause confusion.

This looks like this in practice:

```toml
[dependencies]
foo = "^1.0.0"

[patch.crates-io]
midnight-foo = { git = "https://github.com/midnightntwrk/midnight-ledger", tag = "foo-1.2.3-rc.1" }
```

The following special cases currently exist, but may be revisited in the future:
- `zswap` and `ledger` *will* carry the pre-release suffix in their version. This means that consumers need to specify their version *exactly* for the pre-release.
- `proof-server` and the `*-wasm` crates *also* carry the pre-release suffix, but are assumed to not be directly imported (they are not released on crates.io)
- `ledger` itself has a crate override tag of `crate-ledger-1.2.3-rc.1`, as `ledger-1.2.3-rc.1` is already reserved for the full (non-isolated) state of the repo.

### Isolated crate tags

The above-mentioned crate tags need to be specially crafted, as `git`
resolution by cargo will then prefer the local dependency specification over
the `crates.io` ones. This can lead to unintended dependency duplication, for
instance, if the consumer pulls in both `foo` and `bar` crates from the ledger,
and patches both to `foo-1.2.3-rc.1` and `bar-2.3.4-rc.1`, and `foo` depends on
`bar`, we will end up with two incompatible instances of `bar`: `bar` in the
tag `foo-1.2.3-rc.1` and `bar` in the tag `bar-2.3.4-rc.1`. To prevent this, we
need to *isolate* the crates in their pre-release tag. When releasing `foo`:
- In the root `Cargo.toml`, all crates other than `foo` are commented out.
- In `foo/Cargo.toml`, for all `dependencies` and `dev-dependencies`, `path = "..."` entries are removed (and if necessary, a `version = "..."` entry is added).

This forces `foo-1.2.3-rc.1`'s dependency to be `bar = "^2.0.0"`, which then will *also* be patched to `bar-2.3.4-rc.1`.
Note that this *may not build by itself* if the pre-releases depend on each other. This is fine.

## Backporting and chained releases

As a first preference, a release should be taken from the relevant `ledger-*`
branch. However, in some cases changes need to be included in a prior release
while pulling in minimal noise. For instance:
- Security fixes for prior versions no longer under active development
- Additions to a release candidate, or promoting a release candidate to a full release

In such cases, the basis of the release should be *the prior release*. For
instance, a security fix `7.0.1` would be based on the `ledger-7.0.0` tag. A
pre-release `8.1.0-rc.2` would be based on the prior pre-release
`ledger-8.1.0-rc.1`.

Into this basis, necessary changes should be cherry-picked, and then the
standard release process followed. This should be still opened as a PR into the
relevant branch, and this PR should be merged. This will likely require
resolving conflicts on versioning on the release PR before merging *in favour
of the target branch*. For instance, a `ledger-8.1.0-rc.2` may conflict because
it changes the version to `-rc.2`, while the target branch uses `ledger-8.2.0`.
The target branch will win this conflict, but not until a tag with
`ledger-8.1.0-rc.2` has been cut.
