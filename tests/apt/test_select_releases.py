from __future__ import annotations

import importlib.util
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "select_releases", ROOT / "scripts" / "apt" / "select_releases.py"
)
assert SPEC and SPEC.loader
selector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(selector)


def release(tag: str, published: str, *, complete: bool = True) -> dict[str, object]:
    assets = []
    if complete:
        assets = [
            {"name": "release-manifest.json"},
            {"name": f"le-harness_{tag}_amd64.deb"},
            {"name": f"le-harness_{tag}_arm64.deb"},
        ]
    return {
        "tag_name": tag,
        "published_at": published,
        "draft": False,
        "prerelease": "-" in tag.removeprefix("v"),
        "assets": assets,
    }


class SelectReleasesTests(unittest.TestCase):
    def test_retains_channels_independently_in_publication_order(self) -> None:
        releases = [
            release("v1.1.0-rc.1", "2026-04-04T00:00:00Z"),
            release("v1.0.0", "2026-04-03T00:00:00Z"),
            release("v1.0.0-rc.2", "2026-04-02T00:00:00Z"),
            release("v0.9.0", "2026-04-01T00:00:00Z"),
        ]
        self.assertEqual(
            selector.select(releases, retention=1),
            ["v1.1.0-rc.1", "v1.0.0"],
        )

    def test_incomplete_and_draft_releases_are_not_promoted(self) -> None:
        incomplete = release("v1.0.0-rc.1", "2026-04-02T00:00:00Z", complete=False)
        draft = release("v1.0.0-rc.2", "2026-04-03T00:00:00Z")
        draft["draft"] = True
        complete = release("v1.0.0-rc.3", "2026-04-04T00:00:00Z")
        self.assertEqual(selector.select([incomplete, draft, complete], 3), ["v1.0.0-rc.3"])

    def test_required_release_must_be_complete_and_retained(self) -> None:
        releases = [
            release("v1.0.0-rc.2", "2026-04-02T00:00:00Z"),
            release("v1.0.0-rc.1", "2026-04-01T00:00:00Z"),
        ]
        with self.assertRaisesRegex(ValueError, "not a complete retained"):
            selector.select(releases, retention=1, required_tag="v1.0.0-rc.1")

    def test_prerelease_flag_must_agree_with_tag(self) -> None:
        candidate = release("v1.0.0", "2026-04-01T00:00:00Z")
        candidate["prerelease"] = True
        with self.assertRaisesRegex(ValueError, "channel"):
            selector.select([candidate], retention=3)


if __name__ == "__main__":
    unittest.main()
