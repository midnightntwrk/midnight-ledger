#!/usr/bin/env python3
"""Isolate a single workspace package, tag it, push the tag, and revert.

Usage: scripts/isolate.py <pkg>-<version>   e.g. ledger-9.0.1.0-alpha.1
"""

import pathlib
import re
import subprocess
import sys


def run(*cmd):
    subprocess.run(cmd, check=True)


def main():
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} <pkg>-<version>")
    tag = sys.argv[1]

    m = re.match(r"^(.+?)-(\d.*)$", tag)
    if not m:
        sys.exit(f"could not parse name/version from {tag!r}")
    name = m.group(1)

    repo = pathlib.Path(
        subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    )

    if subprocess.check_output(["git", "status", "--porcelain"], cwd=repo, text=True).strip():
        sys.exit("working tree is dirty; commit or stash first")

    start = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()

    sub_cargo = repo / name / "Cargo.toml"
    if not sub_cargo.is_file():
        sys.exit(f"no Cargo.toml at {sub_cargo}")

    # 1. Filter root Cargo.toml members + default-members down to `name`.
    root = repo / "Cargo.toml"
    text = root.read_text()

    def filter_list(text, key):
        pattern = re.compile(rf"(?ms)^({re.escape(key)}\s*=\s*\[)(.*?)(\])")
        quoted = f'"{name}"'

        def repl(m):
            head, body, tail = m.group(1), m.group(2), m.group(3)
            kept = [line for line in body.splitlines() if quoted in line]
            if not kept:
                sys.exit(f"{name!r} not found in {key}")
            return head + "\n" + "\n".join(kept) + "\n" + tail

        new_text, n = pattern.subn(repl, text)
        if n == 0:
            sys.exit(f"could not find [{key}] list in root Cargo.toml")
        return new_text

    text = filter_list(text, "members")
    text = filter_list(text, "default-members")
    root.write_text(text)

    # 2. Strip path = "..." from [dependencies] and [dev-dependencies] in foo/Cargo.toml.
    targets = {"[dependencies]", "[dev-dependencies]"}
    in_target = False
    out_lines = []
    for line in sub_cargo.read_text().splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_target = stripped in targets
        if in_target:
            line = re.sub(r'path\s*=\s*"[^"]*"\s*,\s*', "", line)
            line = re.sub(r',\s*path\s*=\s*"[^"]*"', "", line)
        out_lines.append(line)
    sub_cargo.write_text("".join(out_lines))

    # 3. Commit, tag, push, revert.
    # `ledger` is the headline package: the bare `ledger-<version>` tag is
    # already used for the release commit, so isolated-crate tags get a
    # `crate-` prefix to disambiguate.
    git_tag = f"crate-{tag}" if name == "ledger" else tag
    msg = f"isolate {git_tag}"
    try:
        run("git", "-C", str(repo), "add", "Cargo.toml", f"{name}/Cargo.toml")
        run("git", "-C", str(repo), "commit", "-m", msg)
        run("git", "-C", str(repo), "tag", "-a", git_tag, "-m", msg)
        run("git", "-C", str(repo), "push", "origin", git_tag)
    finally:
        run("git", "-C", str(repo), "reset", "--hard", start)


if __name__ == "__main__":
    main()
