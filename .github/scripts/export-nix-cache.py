#!/usr/bin/env python3
# This file is part of midnight-ledger.
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0

"""Keep costly built outputs; public substituters supply tools and sources."""

import os
from pathlib import Path
import re
import shutil
import subprocess


def reusable(path):
    name = path.name[33:]
    if name.endswith(("-vendor", "-source")):
        return False
    return path.is_dir() and re.fullmatch(
        r"(?:zkir(?:-static-[a-z0-9_-]+)?|midnight-zkir|midnight-local-params|"
        r"midnight-ledger-test-artifacts|midnight-testing-public-parameters|"
        r"cargo-audit|ledger(?:-static-[a-z0-9_-]+)?|ledger-wasm|"
        r"onchain-runtime-wasm|zkir-wasm)(?:-[0-9].*)?",
        name,
    ) is not None


def main():
    paths = subprocess.check_output(
        ["nix", "path-info", "--all"], text=True
    ).splitlines()
    roots = [path for path in paths if reusable(Path(path))]
    if not roots:
        print("No reusable outputs were realized; leaving the restored cache intact.")
        return
    cache = Path(os.environ["RUNNER_TEMP"]) / "nix-ci-cache"
    staging = cache.with_name("nix-ci-cache-next")
    shutil.rmtree(staging, ignore_errors=True)
    subprocess.run(
        ["nix", "copy", "--to", f"file://{staging}?compression=zstd", *roots],
        check=True,
    )
    # Replacing rather than extending drops outputs unused by this revision.
    shutil.rmtree(cache, ignore_errors=True)
    staging.rename(cache)
    size = sum(path.stat().st_size for path in cache.rglob("*") if path.is_file())
    if "GITHUB_OUTPUT" in os.environ:
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write("has-outputs=true\n")
    print(f"Cached {len(roots)} output roots; compressed binary cache: {size / 2**20:.1f} MiB")


if __name__ == "__main__":
    main()
