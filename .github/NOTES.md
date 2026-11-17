# scripts/

Reusable scripts for use in workflows/*.yml files.

# renovate.json

The renovate config file doesn't allow comments, so documenting here :/

We're grouping PRs for the same package for all patch-version semver bumps via this part of the config (https://docs.renovatebot.com/configuration-options/#groupname):

```json
{
  "groupName": "devDependencies (patch)",
  "matchDepTypes": [
    "devDependencies"
  ],
  "matchUpdateTypes": [
    "patch"
  ],
  "automerge": true
},
{
  "groupName": "dependencies (patch)",
  "matchDepTypes": [
    "dependencies"
  ],
  "matchUpdateTypes": [
    "patch"
  ]
}
```

However, this has the side effect that renovate creates "immortal" PRs
(https://docs.renovatebot.com/key-concepts/pull-requests/#immortal-prs) for
these groups, which means by default we can't close them without merging them,
because they'll just be recreated (so more like "zombie" PRs ...). So, we set
the top-level `"recreateWhen": "never"` to allow closing these grouped PRs
without merging.

# CI build reuse

The `cache-nix` and `save-nix` actions must surround the build steps in a job.
They cache compressed runtime closures of expensive project outputs, rather
than `/nix/store` with its derivations, sources and downloaded toolchains.
Keys include OS, architecture, workload, Nix configuration and commit; fallback
keys allow reuse across commits without competing writers or destructive purges.
Only the restored local file cache is trusted; public substituters retain their
signature checks. `save-nix` also retains completed outputs after a test failure.
Base-branch CI and coverage runs populate caches accessible to later PRs.

`export-integration-deps` builds the three WASM packages and proof server once.
The integration action imports that runtime closure from the same workflow run,
then asks Nix to realize the expected output before executing the tests. Release
coverage and native unit tests run alongside package builds; integration waits
for its dependency artifact. Existing publication dependencies remain intact.

Coverage uses a separate Cargo cache and four test threads, except proof-server
tests stay serial to avoid contention against HTTP deadlines. It still executes
all workspace features and produces both HTML and LCOV reports. Native PR tests
retain their existing serial behavior. Custom larger-runner labels are managed
by the organization; ordinary PR runner images are pinned while the legacy
check names are preserved for branch protection.

Validate workflow edits with `actionlint -shellcheck=""`, run the Python helper
checks with `python3 -m unittest discover -s .github/scripts/tests`, and compare
cold/warm CI runs. Check the exported cache sizes, actual cache hits, coverage
test counts and the integration consumer's build log, not just job conclusions.
