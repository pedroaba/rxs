"""Reject inconsistent release tags before building or publishing artifacts."""
import json
import plistlib
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def validate(tag, version, bundle_version):
    number = r"(?:0|[1-9][0-9]*)"
    identifier = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    pattern = rf"v({number}\.{number}\.{number})(?:-{identifier}(?:\.{identifier})*)?"
    match = re.fullmatch(pattern, tag)
    if not match:
        raise ValueError("Use a tag like v0.1.0 or v0.2.0-rc.1 (without build metadata)")
    if tag[1:] != version:
        raise ValueError(f"Tag {tag} does not match Cargo.toml version {version}")
    if match[1] != bundle_version:
        raise ValueError(
            f"Info.plist version {bundle_version} must match release base version {match[1]}"
        )


def main():
    if len(sys.argv) != 2:
        raise ValueError("Usage: python3 scripts/validate-release.py v0.1.0")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"], cwd=ROOT
    ))
    package = next(p for p in metadata["packages"] if p["name"] == "rxs")
    with (ROOT / "packaging/Info.plist").open("rb") as source:
        plist = plistlib.load(source)
    validate(sys.argv[1], package["version"], plist["CFBundleShortVersionString"])
    print(f"Release version verified: {sys.argv[1]}")


if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        sys.exit(str(error))
