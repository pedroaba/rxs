"""Installer failure paths must never remove the user's existing app."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("rsx_signing", Path(__file__).parents[1] / "scripts/signing.py")
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)


class SigningTests(unittest.TestCase):
    def test_missing_identity_fails_before_native_signing(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(signing, "CONFIG", Path(temporary) / "missing.json"), patch.object(signing, "run") as native:
                with self.assertRaisesRegex(RuntimeError, "ausente"):
                    signing.load_identity()
                native.assert_not_called()

    def exercise_failure(self, mode):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, target = root / "candidate.app", root / "RSX.app"
            source.mkdir()
            target.mkdir()
            (target / "original").write_text("existing user installation")
            rename = Path.rename

            def native(*args, **_kwargs):
                if args[0] == "/usr/bin/ditto":
                    Path(args[-1]).mkdir()
                    (Path(args[-1]) / "new").write_text("new installation")
                if "-R" in args and mode == "identity":
                    raise RuntimeError("incompatible identity")
                return ""

            def validate(app):
                if app == target and mode == "verify":
                    raise RuntimeError("invalid installed bundle")
                return "stable identity"

            def guarded_rename(path, destination):
                if path == target and mode == "rename":
                    raise OSError("destination unavailable")
                return rename(path, destination)

            with patch.object(signing, "load_identity", return_value=("A" * 40, "local.keychain")), patch.object(signing, "validate", side_effect=validate), patch.object(signing, "bundle_identifier", return_value=signing.BUNDLE_ID), patch.object(signing, "info", return_value="stable identity"), patch.object(signing, "requirement", return_value="stable identity"), patch.object(signing, "installed_pids", return_value=[]), patch.object(signing, "run", side_effect=native), patch.object(Path, "rename", guarded_rename):
                with self.assertRaises((RuntimeError, OSError)):
                    signing.install(source, target, False)
            self.assertEqual((target / "original").read_text(), "existing user installation")
            self.assertFalse((target / "new").exists())

    def test_incompatible_identity_never_replaces_existing_app(self):
        self.exercise_failure("identity")

    def test_failed_installed_verification_restores_original(self):
        self.exercise_failure("verify")

    def test_failed_backup_rename_does_not_delete_original(self):
        self.exercise_failure("rename")


if __name__ == "__main__":
    unittest.main()
