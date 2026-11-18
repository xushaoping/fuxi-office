#!/usr/bin/env python3
"""Verify the CSR release bundle and bind its assets, executable and source revision."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

REQUIRED_ASSETS = ("pkg/fuxi-dmn.js", "pkg/fuxi-dmn.wasm", "pkg/fuxi-dmn.css")


def file_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def asset_inventory(site):
    if not site.is_dir():
        raise ValueError("release site directory is missing")
    entries = []
    for path in sorted(site.rglob("*")):
        if path.is_symlink():
            raise ValueError("release assets must not contain symbolic links")
        if not path.is_file() or path.relative_to(site).as_posix() in {"index.html", "release-manifest.json"}:
            continue
        entries.append({"path": path.relative_to(site).as_posix(),
                        "size": path.stat().st_size, "sha256": file_digest(path)})
    by_name = {item["path"]: item for item in entries}
    for name in REQUIRED_ASSETS:
        if name not in by_name or by_name[name]["size"] == 0:
            raise ValueError(f"release asset is missing or empty: {name}")
    with (site / "pkg/fuxi-dmn.wasm").open("rb") as source:
        if source.read(8) != b"\x00asm\x01\x00\x00\x00":
            raise ValueError("release WASM has an invalid header/version")
    raw = json.dumps(entries, sort_keys=True, separators=(",", ":")).encode()
    return entries, hashlib.sha256(raw).hexdigest()


def source_revision(root):
    try:
        commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root,
                                capture_output=True, text=True, check=True).stdout.strip()
        status = subprocess.run(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=root,
                                capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        # Docker deliberately excludes .git; the trusted build pipeline supplies
        # these from its checked-out revision. This is provenance, not runtime acceptance.
        supplied = os.environ.get("FUXI_BUILD_COMMIT", "")
        if re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", supplied):
            return {"commit": supplied, "workingTreeClean": os.environ.get("FUXI_SOURCE_CLEAN") == "1",
                    "source": "build_pipeline_metadata"}
        return {"commit": None, "workingTreeClean": False, "source": "unavailable"}
    return {"commit": commit, "workingTreeClean": not status.strip(), "source": "git_checkout"}


def verify_bundle(site, binary, root, require_clean=False):
    assets, version = asset_inventory(site)
    index = site / "index.html"
    if not index.is_file() or index.is_symlink():
        raise ValueError("release index.html is missing or is a symbolic link")
    html = index.read_text(encoding="utf-8")
    if "__FUXI_ASSET_VERSION__" in html:
        raise ValueError("release index.html contains an unresolved asset version")
    for name in REQUIRED_ASSETS:
        if f'/{name}?v={version}' not in html:
            raise ValueError(f"release shell does not bind the current asset digest: {name}")
    if not binary.is_file() or binary.is_symlink() or not os.access(binary, os.X_OK):
        raise ValueError("release executable is missing, not executable or a symbolic link")
    revision = source_revision(root)
    if require_clean and (not revision["commit"] or not revision["workingTreeClean"]):
        raise ValueError("production candidate must come from a clean, tracked Git revision")
    return {"schemaVersion": 1, "assetVersion": version, "source": revision,
            "productionSourceEligible": bool(revision["commit"] and revision["workingTreeClean"]),
            "binary": {"name": binary.name, "size": binary.stat().st_size, "sha256": file_digest(binary)},
            "indexSha256": file_digest(index), "assets": assets,
            "scope": "artifact integrity only; runtime, browser and business acceptance remain separate"}


def write_manifest(path, report):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=f".{path.name}.", delete=False) as target:
            temporary = target.name
            json.dump(report, target, ensure_ascii=False, indent=2)
            target.write("\n")
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--site-root", type=Path, default=Path("target/site"))
    parser.add_argument("--binary", type=Path, default=Path("target/release/fuxi-dmn"))
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--stamp-only", action="store_true", help="print the built asset digest before shell/executable packaging")
    parser.add_argument("--require-clean", action="store_true")
    parser.add_argument("--output", type=Path, help="manifest destination; defaults to stdout")
    args = parser.parse_args(argv)
    try:
        if args.stamp_only:
            if args.require_clean or args.output:
                raise ValueError("--stamp-only cannot be combined with candidate/manifest options")
            print(asset_inventory(args.site_root)[1])
            return 0
        report = verify_bundle(args.site_root, args.binary, args.repo_root, args.require_clean)
        if args.output:
            write_manifest(args.output, report)
        else:
            print(json.dumps(report, ensure_ascii=False, indent=2))
    except (OSError, ValueError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
