from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


def load(name: str):
    path = ROOT / "scripts" / "release" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


validate_release = load("validate_release")
assemble_manifest = load("assemble_manifest")


class ValidateReleaseTests(unittest.TestCase):
    def test_current_package_metadata_is_consistent(self) -> None:
        result = validate_release.validate(ROOT, "v0.1.0-rc.3")
        self.assertEqual(result["version"], "0.1.0-rc.3")
        self.assertEqual(result["channel"], "preview")
        self.assertTrue(result["prerelease"])

    def test_mismatched_tag_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "does not match"):
            validate_release.validate(ROOT, "v9.9.9")


class AssembleManifestTests(unittest.TestCase):
    def test_manifest_contains_verified_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            for target, extension in assemble_manifest.TARGETS.items():
                (directory / f"le-harness-v1.2.3-{target}.{extension}").write_bytes(
                    target.encode()
                )
            for target, architecture in assemble_manifest.DEBIAN_ARCHITECTURES.items():
                (directory / f"le-harness_1.2.3-1_{architecture}.deb").write_bytes(
                    target.encode()
                )

            manifest = assemble_manifest.assemble(directory, "v1.2.3", "a" * 40)

            self.assertEqual(manifest["channel"], "stable")
            self.assertEqual(len(manifest["artifacts"]), 6)
            serialized = json.dumps(manifest)
            self.assertIn("sha256", serialized)
            self.assertIn("source_commit", serialized)

    def test_missing_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, "required artifact"):
                assemble_manifest.assemble(pathlib.Path(temporary), "v1.0.0", "b" * 40)

    def test_debian_prerelease_versions_sort_before_stable(self) -> None:
        self.assertEqual(
            assemble_manifest.debian_version("0.1.0-rc.3"), "0.1.0~rc.3-1"
        )
        self.assertEqual(assemble_manifest.debian_version("0.1.0"), "0.1.0-1")


if __name__ == "__main__":
    unittest.main()
