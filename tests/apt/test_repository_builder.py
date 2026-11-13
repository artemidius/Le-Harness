from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "build_repository", ROOT / "scripts" / "apt" / "build_repository.py"
)
assert SPEC and SPEC.loader
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class RepositoryBuilderTests(unittest.TestCase):
    def test_suite_parser_accepts_safe_names(self) -> None:
        suite, directory = builder.parse_suite("preview=.")
        self.assertEqual(suite, "preview")
        self.assertTrue(directory.is_absolute())

    def test_suite_parser_rejects_path_like_names(self) -> None:
        with self.assertRaises(Exception):
            builder.parse_suite("../stable=.")

    def test_nonempty_output_is_rejected_before_writes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = pathlib.Path(temporary)
            (output / "keep").write_text("owned", encoding="utf-8")
            arguments = type(
                "Arguments",
                (),
                {
                    "output": output,
                    "suite": [],
                    "architectures": "amd64",
                    "valid_days": 30,
                    "max_bytes": 1,
                    "signing_key": "missing",
                },
            )()
            with self.assertRaisesRegex(ValueError, "absent or empty"):
                builder.build(arguments)
            self.assertEqual((output / "keep").read_text(encoding="utf-8"), "owned")


if __name__ == "__main__":
    unittest.main()
