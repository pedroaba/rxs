#!/usr/bin/env python3
"""Explicit native signing test: tiny synthetic executables, no screen access."""
import importlib.util
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile

spec = importlib.util.spec_from_file_location("rsx_signing", Path(__file__).parents[1] / "scripts/signing.py")
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)

with tempfile.TemporaryDirectory(prefix="rsx-identity-check-") as directory:
    root = Path(directory)
    apps = []
    for version in (1, 2):
        app = root / f"version-{version}.app"
        macos = app / "Contents/MacOS"
        macos.mkdir(parents=True)
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleIdentifier": signing.BUNDLE_ID,
            "CFBundleExecutable": "rsx", "CFBundlePackageType": "APPL",
            "CFBundleVersion": str(version),
        }))
        source = root / f"version-{version}.c"
        source.write_text(f"int main(void) {{ return {version}; }}\n")
        subprocess.run(["/usr/bin/clang", str(source), "-o", str(macos / "rsx")], check=True)
        signing.sign(app)
        apps.append(app)
    assert (apps[0] / "Contents/MacOS/rsx").read_bytes() != (apps[1] / "Contents/MacOS/rsx").read_bytes()
    assert signing.requirement(apps[0]) == signing.requirement(apps[1])
    signing.run("/usr/bin/codesign", "--verify", "--strict", "-R", "=" + signing.requirement(apps[0]), str(apps[1]))
    installed = root / "installed/RSX.app"
    installed.parent.mkdir()
    shutil.copytree(apps[0], installed)
    signing.install(apps[1], installed, False)
    assert (installed / "Contents/MacOS/rsx").read_bytes() == (apps[1] / "Contents/MacOS/rsx").read_bytes()
    # A bundle with the same certificate but a different signing identifier is
    # incompatible with the existing identity; installation must stop untouched.
    foreign = root / "foreign.app"
    shutil.copytree(apps[1], foreign)
    fingerprint, keychain = signing.load_identity()
    signing.run("/usr/bin/codesign", "--force", "--timestamp=none", "--keychain", keychain,
                "--sign", fingerprint, "--identifier", "app.rxs.incompatible", str(foreign))
    before = (installed / "Contents/MacOS/rsx").read_bytes()
    try:
        signing.install(foreign, installed, False)
        raise AssertionError("incompatible update accepted")
    except RuntimeError:
        pass
    assert (installed / "Contents/MacOS/rsx").read_bytes() == before
    # An ad-hoc bundle must never enter the normal installation path.
    signing.run("/usr/bin/codesign", "--force", "--sign", "-", str(foreign))
    try:
        signing.install(foreign, installed, False)
        raise AssertionError("ad-hoc update accepted")
    except RuntimeError:
        pass
    assert (installed / "Contents/MacOS/rsx").read_bytes() == before
    previous = os.environ.get("RSX_SIGNING_CONFIG")
    try:
        signing.CONFIG = root / "missing-signing.json"
        try:
            signing.sign(apps[1])
            raise AssertionError("missing identity accepted")
        except RuntimeError:
            pass
    finally:
        if previous:
            os.environ["RSX_SIGNING_CONFIG"] = previous
print("Two distinct executables share a stable identity; incompatible, ad-hoc and missing-identity updates were rejected.")
