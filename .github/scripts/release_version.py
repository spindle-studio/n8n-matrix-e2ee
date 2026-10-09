"""Validate component release tags against manifests and lockfiles (Python 3.11+)."""

import json
import os
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
# Build metadata (+...) is excluded because it cannot be used in Docker tags.
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?", re.ASCII)


def validate(component, tag, root=ROOT):
    if component not in {"nodes", "gateway"}:
        raise ValueError(f"Unknown component: {component}")
    prefix = f"{component}-v"
    if not tag.startswith(prefix):
        raise ValueError(f"Expected a {prefix}<version> tag, got {tag!r}")
    version = tag[len(prefix):]
    match = VERSION.fullmatch(version)
    if not match or len(version) > 128:
        raise ValueError(f"Invalid release version: {version!r}")
    prerelease = match.group(4)
    if prerelease and any(p.isdigit() and len(p) > 1 and p.startswith("0") for p in prerelease.split(".")):
        raise ValueError("Numeric prerelease identifiers must not have leading zeros")

    if component == "nodes":
        directory = root / "nodes"
        manifest = json.loads((directory / "package.json").read_text())
        lock = json.loads((directory / "package-lock.json").read_text())
        versions = [manifest["version"], lock["version"], lock["packages"][""]["version"]]
    else:
        manifest = tomllib.loads((root / "gateway/Cargo.toml").read_text())
        lock = tomllib.loads((root / "gateway/Cargo.lock").read_text())
        packages = [p for p in lock["package"] if p["name"] == manifest["package"]["name"] and "source" not in p]
        if len(packages) != 1:
            raise ValueError("Expected exactly one gateway entry in Cargo.lock")
        versions = [manifest["package"]["version"], packages[0]["version"]]
    if any(v != version for v in versions):
        raise ValueError(f"Tag version {version} does not match manifest/lockfile versions {versions}")
    return {"version": version, "stable": str(prerelease is None).lower(), "npm_tag": "next" if prerelease else "latest"}


def main():
    try:
        result = validate(sys.argv[1], sys.argv[2])
    except (ValueError, KeyError, IndexError, OSError) as error:
        print(f"Release validation failed: {error}", file=sys.stderr)
        return 1
    output = "".join(f"{key}={value}\n" for key, value in result.items())
    print(output, end="")
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a") as handle:
            handle.write(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
