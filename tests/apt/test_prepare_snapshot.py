from __future__ import annotations

import hashlib
import importlib.util
import json
import pathlib
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "prepare_snapshot", ROOT / "scripts" / "apt" / "prepare_snapshot.py"
)
assert SPEC and SPEC.loader
snapshot = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(snapshot)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class PrepareSnapshotTests(unittest.TestCase):
    def fixture(self, directory: pathlib.Path) -> None:
        artifacts = []
        for architecture in ["amd64", "arm64"]:
            name = f"le-harness_0.1.0~rc.3-1_{architecture}.deb"
            data = architecture.encode()
            (directory / name).write_bytes(data)
            artifacts.append(
                {
                    "kind": "deb",
                    "name": name,
                    "architecture": architecture,
                    "size": len(data),
                    "sha256": sha256(data),
                }
            )
        (directory / "release-manifest.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "tag": "v0.1.0-rc.3",
                    "version": "0.1.0-rc.3",
                    "channel": "preview",
                    "artifacts": artifacts,
                }
            ),
            encoding="utf-8",
        )

    def test_verified_packages_are_staged_by_channel(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            released = root / "released"
            released.mkdir()
            self.fixture(released)
            metadata = snapshot.prepare(released, root / "prepared")
            self.assertEqual(metadata["suite"], "preview")
            self.assertEqual(len(list((root / "prepared" / "preview").glob("*.deb"))), 2)

    def test_changed_package_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            released = root / "released"
            released.mkdir()
            self.fixture(released)
            next(released.glob("*.deb")).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "does not match"):
                snapshot.prepare(released, root / "prepared")

    def test_channel_version_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            released = root / "released"
            released.mkdir()
            self.fixture(released)
            manifest_path = released / "release-manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["channel"] = "stable"
            manifest_path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, "channel"):
                snapshot.prepare(released, root / "prepared")

    def test_duplicate_release_is_rejected_before_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            released = root / "released"
            released.mkdir()
            self.fixture(released)
            output = root / "prepared"
            with self.assertRaisesRegex(ValueError, "duplicate tags"):
                snapshot.prepare_many([released, released], output)
            self.assertFalse(output.exists())

    def test_retention_is_enforced_before_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            released = root / "released"
            released.mkdir()
            self.fixture(released)
            second = root / "second"
            second.mkdir()
            self.fixture(second)
            manifest_path = second / "release-manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["tag"] = "v0.1.0-rc.4"
            manifest["version"] = "0.1.0-rc.4"
            manifest_path.write_text(json.dumps(manifest))
            output = root / "prepared"
            with self.assertRaisesRegex(ValueError, "retention"):
                snapshot.prepare_many([released, second], output, retention=1)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
