#!/usr/bin/env python3
"""Check that a clean Git revision is ready to be frozen as a versioned candidate."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys


STABLE_TAG = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def run_git(root, *args):
    return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=True)


def package_version(root):
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--manifest-path",
         str(root / "Cargo.toml")], cwd=root, capture_output=True, text=True, check=True)
    metadata = json.loads(result.stdout)
    root_manifest = (root / "Cargo.toml").resolve()
    for package in metadata.get("packages", []):
        if Path(package.get("manifest_path", "")).resolve() == root_manifest:
            return package["version"]
    raise ValueError("Cargo metadata did not report the root package")


def check_candidate(root, release_version, require_tag=False):
    if not VERSION.fullmatch(release_version):
        raise ValueError("release version must use stable MAJOR.MINOR.PATCH form")
    cargo_version = package_version(root)
    if cargo_version != release_version:
        raise ValueError(f"Cargo package version {cargo_version} does not match {release_version}")

    current = tuple(map(int, release_version.split(".")))
    tags = run_git(root, "tag", "--list", "v*").stdout.splitlines()
    stable_tags = []
    for tag in tags:
        match = STABLE_TAG.fullmatch(tag)
        if match:
            stable_tags.append((tuple(map(int, match.groups())), tag))
    latest = max(stable_tags, default=None)
    selected_tag = "v" + release_version
    if latest and current <= latest[0] and not (require_tag and selected_tag == latest[1]):
        raise ValueError(f"release version must be higher than the latest stable tag {latest[1]}")

    commit = run_git(root, "rev-parse", "HEAD").stdout.strip()
    dirty = run_git(root, "status", "--porcelain", "--untracked-files=all").stdout
    if dirty:
        raise ValueError("release candidate worktree must be clean; commit all intended source, migrations, tests, CI and docs")

    tag_commit = None
    if require_tag:
        try:
            tag_commit = run_git(root, "rev-parse", f"refs/tags/{selected_tag}^{{commit}}").stdout.strip()
        except subprocess.CalledProcessError as error:
            raise ValueError(f"required release tag {selected_tag} is missing") from error
        if tag_commit != commit:
            raise ValueError(f"release tag {selected_tag} does not point to HEAD")

    return {"status": "passed", "version": release_version, "tag": selected_tag,
            "commit": commit, "latestStableTag": latest[1] if latest else None,
            "tagVerified": require_tag, "productionSourceEligible": True}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True, help="selected stable release version, e.g. 0.2.0")
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--require-tag", action="store_true",
                        help="require v<version> to exist and point to HEAD after candidate acceptance")
    args = parser.parse_args(argv)
    try:
        report = check_candidate(args.repo_root.resolve(), args.version, args.require_tag)
    except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"release candidate check failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
