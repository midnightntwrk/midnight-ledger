#!/usr/bin/env bash
# Publish an npm tarball idempotently.
#
# Re-runs are safe: if the version already exists on the target registry,
# the publish is skipped (exit 0). Optionally, when FORCE=true, the version
# is deleted from GH Packages before publishing - public npm cannot be
# overwritten so FORCE on registry.npmjs.org errors out.
#
# Args:
#   $1  tgz path           e.g. midnight-ledger-v9-0.1.0-alpha.1.tgz
#   $2  registry URL       e.g. https://npm.pkg.github.com/midnightntwrk
#   $3  package full name  e.g. @midnightntwrk/ledger-v9
#   $4  package version    e.g. 0.1.0-alpha.1
#
# The dist-tag is derived from the version's semver pre-release identifier:
# release versions (no suffix) → 'latest'; pre-releases → identifier
# (alpha/beta/rc/next/...). `npm dist-tag add` is never called from CI;
# the version string is the single source of truth.
#
# Env:
#   FORCE      "true" to attempt overwrite (GH Packages only).
#   GH_TOKEN   required when FORCE=true on GH Packages, with delete:packages.
set -euo pipefail

TGZ="$1"
REG="$2"
PKG="$3"
VER="$4"

if [[ "$VER" =~ ^[0-9]+\.[0-9]+\.[0-9]+-([a-zA-Z][a-zA-Z0-9_-]*) ]]; then
  TAG="${BASH_REMATCH[1]}"
else
  TAG="latest"
fi

REG_HOST="${REG#https://}"; REG_HOST="${REG_HOST%%/*}"

# Scope of the package, e.g. "@midnightntwrk". setup-node writes a
# `<scope>:registry=...` line into .npmrc, and for scoped packages npm resolves
# the registry from that mapping and IGNORES --registry. Override the scope
# registry explicitly so view/publish actually target $REG.
SCOPE="${PKG%%/*}"
SCOPE_REG_ARG="--${SCOPE}:registry=$REG"

is_published() {
  npm view "$PKG@$VER" version --registry="$REG" "$SCOPE_REG_ARG" >/dev/null 2>&1
}

if [ "${FORCE:-false}" = "true" ]; then
  case "$REG_HOST" in
    npm.pkg.github.com)
      # @scope/name -> scope=midnightntwrk name=ledger-v9
      ORG="${PKG#@}"; ORG="${ORG%%/*}"
      NAME="${PKG##*/}"
      echo "force=true: looking up $PKG@$VER on GH Packages"
      VERSION_ID=$(gh api "/orgs/$ORG/packages/npm/$NAME/versions" \
        --jq ".[] | select(.name==\"$VER\") | .id" 2>/dev/null || true)
      if [ -n "$VERSION_ID" ]; then
        echo "Deleting $PKG@$VER (id=$VERSION_ID)"
        gh api -X DELETE "/orgs/$ORG/packages/npm/$NAME/versions/$VERSION_ID"
      else
        echo "No existing version found; proceeding to publish"
      fi
      ;;
    registry.npmjs.org)
      if is_published; then
        echo "ERROR: FORCE=true but $PKG@$VER already exists on public npm." >&2
        echo "Public npm versions are immutable. Bump the version and re-run." >&2
        exit 1
      fi
      ;;
    *)
      echo "FORCE=true: unsupported registry host '$REG_HOST'" >&2
      exit 1
      ;;
  esac
fi

if is_published; then
  echo "Already published: $PKG@$VER on $REG_HOST - skipping"
  exit 0
fi

publish_args=(--registry="$REG" "$SCOPE_REG_ARG" --tag "$TAG")
# Scoped packages publish as 'restricted' by default on public npm; make the
# first publish public. (GH Packages visibility is governed by the repo/org.)
if [ "$REG_HOST" = "registry.npmjs.org" ]; then
  publish_args+=(--access public)
fi
set +e
OUTPUT=$(npm publish "$TGZ" "${publish_args[@]}" 2>&1)
STATUS=$?
set -e
printf '%s\n' "$OUTPUT"
if [ "$STATUS" -ne 0 ]; then
  # A conflict means the version is already there (race, or a registry whose
  # metadata endpoint was not readable by this token) - that is the idempotent
  # success condition, not a failure.
  # Anchored on 'code E409': npm's shasum/integrity notices are hex/base64
  # that a bare 'E409' would occasionally match.
  if grep -qiE 'code E409|EPUBLISHCONFLICT|cannot publish over' <<<"$OUTPUT"; then
    echo "Version already exists on $REG_HOST (registry reported a conflict) - skipping"
    exit 0
  fi
  exit "$STATUS"
fi
