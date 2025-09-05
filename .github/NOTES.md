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
