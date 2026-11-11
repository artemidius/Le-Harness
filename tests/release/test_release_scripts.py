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
render_homebrew_formula = load("render_homebrew_formula")


class ValidateReleaseTests(unittest.TestCase):
    def test_current_package_metadata_is_consistent(self) -> None:
        result = validate_release.validate(ROOT, "v0.1.0-rc.7")
        self.assertEqual(result["version"], "0.1.0-rc.7")
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
                (directory / assemble_manifest.debian_asset_name("1.2.3", architecture)).write_bytes(
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

    def test_debian_release_asset_name_is_not_rewritten_by_github(self) -> None:
        self.assertEqual(
            assemble_manifest.debian_asset_name("0.1.0-rc.4", "amd64"),
            "le-harness_0.1.0-rc.4-1_amd64.deb",
        )
        self.assertEqual(assemble_manifest.debian_version("0.1.0"), "0.1.0-1")


class RenderHomebrewFormulaTests(unittest.TestCase):
    def fixture(self, directory: pathlib.Path, version: str = "1.2.3-rc.4") -> pathlib.Path:
        artifacts = []
        for target in render_homebrew_formula.TARGETS:
            name = f"le-harness-v{version}-{target}.tar.gz"
            asset = directory / name
            asset.write_bytes(target.encode())
            artifacts.append(
                {
                    "kind": "archive",
                    "name": name,
                    "target": target,
                    "size": asset.stat().st_size,
                    "sha256": render_homebrew_formula.digest(asset),
                }
            )
        manifest = directory / "release-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "tag": f"v{version}",
                    "version": version,
                    "channel": "preview" if "-" in version else "stable",
                    "source_commit": "a" * 40,
                    "artifacts": artifacts,
                }
            ),
            encoding="utf-8",
        )
        return manifest

    def test_verified_manifest_renders_immutable_urls_and_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            manifest = self.fixture(directory)
            version, archives = render_homebrew_formula.load_archives(manifest, directory)
            formula = render_homebrew_formula.render(version, archives)

            self.assertIn("releases/download/v1.2.3-rc.4", formula)
            self.assertIn("MISTRAL_API_KEY environment override: missing", formula)
            for artifact in archives.values():
                self.assertIn(str(artifact["sha256"]), formula)

    def test_tampered_asset_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            manifest = self.fixture(directory)
            asset = next(directory.glob("*.tar.gz"))
            asset.write_bytes(b"changed bytes")

            with self.assertRaisesRegex(ValueError, "mismatch"):
                render_homebrew_formula.load_archives(manifest, directory)

    def test_missing_target_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            manifest = self.fixture(directory)
            data = json.loads(manifest.read_text(encoding="utf-8"))
            data["artifacts"].pop()
            manifest.write_text(json.dumps(data), encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "missing Homebrew targets"):
                render_homebrew_formula.load_archives(manifest, directory)

    def test_release_ordering_prevents_rollback(self) -> None:
        version = render_homebrew_formula.Version.parse
        self.assertLess(version("1.2.3-rc.4"), version("1.2.3"))
        self.assertLess(version("1.2.3-rc.4"), version("1.2.3-rc.10"))
        self.assertLess(version("1.2.3"), version("1.3.0-rc.1"))

    def test_current_formula_requires_one_consistent_version(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            formula = pathlib.Path(temporary) / "le-harness.rb"
            formula.write_text(
                'url "https://github.com/artemidius/Le-Harness/releases/download/'
                'v1.2.3/le-harness-v1.2.3-aarch64-apple-darwin.tar.gz"\n',
                encoding="utf-8",
            )
            self.assertEqual(render_homebrew_formula.current_version(formula), "1.2.3")


if __name__ == "__main__":
    unittest.main()
