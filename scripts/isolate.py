#!/usr/bin/env python3
"""Isolate a single workspace package, tag it, push the tag, and revert.

Usage: scripts/isolate.py [-f] <pkg>-<version>   e.g. ledger-9.0.1.0-alpha.1
"""

import argparse
import pathlib
import re
import subprocess
import sys


def run(*cmd):
    subprocess.run(cmd, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("tag", help="<pkg>-<version>, e.g. ledger-9.0.1.0-alpha.1")
    parser.add_argument(
        "-f", "--force", action="store_true", help="overwrite an existing tag locally and on origin"
    )
    args = parser.parse_args()
    tag = args.tag

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

    def rewrite_list(text, key, require_member):
        pattern = re.compile(rf"(?ms)^({re.escape(key)}\s*=\s*\[)(.*?)(\])")
        quoted = f'"{name}"'

        def repl(m):
            head, tail = m.group(1), m.group(3)
            if require_member and quoted not in m.group(2):
                sys.exit(f"{name!r} not found in {key}")
            return head + "\n    " + quoted + ",\n" + tail

        new_text, n = pattern.subn(repl, text)
        if n == 0:
            sys.exit(f"could not find [{key}] list in root Cargo.toml")
        return new_text

    # `name` must really be a workspace member, but it need not be a
    # *default* member (e.g. `zkir-v3`), so default-members is overwritten
    # rather than filtered.
    text = rewrite_list(text, "members", require_member=True)
    text = rewrite_list(text, "default-members", require_member=False)
    root.write_text(text)

    # 2. In foo/Cargo.toml: strip `path = "..."` from [dependencies], and drop
    # [dev-dependencies] entirely. Override consumers (`[patch]` git deps) never
    # read dev-deps, so dropping the section avoids needing to mirror local-only
    # crate versions here.
    section = None
    out_lines = []
    for line in sub_cargo.read_text().splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped
        if section == "[dev-dependencies]":
            continue
        if section == "[dependencies]":
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
    tag_cmd = ["git", "-C", str(repo), "tag", "-a", git_tag, "-m", msg]
    push_cmd = ["git", "-C", str(repo), "push", "origin", git_tag]
    if args.force:
        tag_cmd.insert(-2, "-f")
        push_cmd.insert(-2, "--force")
    try:
        run("git", "-C", str(repo), "add", "Cargo.toml", f"{name}/Cargo.toml")
        run("git", "-C", str(repo), "commit", "-m", msg)
        run(*tag_cmd)
        run(*push_cmd)
    finally:
        run("git", "-C", str(repo), "reset", "--hard", start)


if __name__ == "__main__":
    main()
