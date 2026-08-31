import importlib.util
import stat
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "prepare_release.py"
SPEC = importlib.util.spec_from_file_location("prepare_release", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT}")
prepare_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare_release)


class ReleaseTagTests(unittest.TestCase):
    def test_accepts_stable_and_prerelease_tags(self):
        self.assertEqual(prepare_release.parse_tag_version("v1.2.3"), "1.2.3")
        self.assertEqual(prepare_release.parse_tag_version("v1.2.3-rc.1"), "1.2.3-rc.1")

    def test_rejects_noncanonical_tags(self):
        for tag in ["1.2.3", "v1.2", "v01.2.3", "v1.2.3-01", "v1.2.3+build"]:
            with (
                self.subTest(tag=tag),
                self.assertRaisesRegex(ValueError, "valid release tag"),
            ):
                prepare_release.parse_tag_version(tag)


class ReleasePreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "wlapse"\nversion = "1.2.3"\n', encoding="utf-8"
        )
        (self.root / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "wlapse"\nversion = "1.2.3"\n',
            encoding="utf-8",
        )
        (self.root / "src").mkdir()
        (self.root / "src" / "lib.rs").write_text("", encoding="utf-8")
        (self.root / "README.md").write_text("# wlapse\n", encoding="utf-8")
        (self.root / "LICENSE").write_text("MIT\n", encoding="utf-8")
        self.binary = self.root / "wlapse"
        self.binary.write_text(
            "#!/bin/sh\nprintf '%s\\n' 'wlapse 1.2.3'\n", encoding="utf-8"
        )
        self.binary.chmod(0o755)

    def tearDown(self):
        self.temporary_directory.cleanup()

    def test_rejects_tag_that_differs_from_manifest(self):
        with self.assertRaisesRegex(ValueError, "does not match Cargo.toml"):
            prepare_release.prepare_release(
                root=self.root,
                tag="v1.2.4",
                target="x86_64-unknown-linux-gnu",
                binary=self.binary,
                output_directory=self.root / "dist",
            )

    def test_rejects_binary_that_reports_another_version(self):
        self.binary.write_text(
            "#!/bin/sh\nprintf '%s\\n' 'wlapse 9.9.9'\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "binary version"):
            prepare_release.prepare_release(
                root=self.root,
                tag="v1.2.3",
                target="x86_64-unknown-linux-gnu",
                binary=self.binary,
                output_directory=self.root / "dist",
            )

    def test_creates_archive_and_matching_checksum(self):
        archive, checksums = prepare_release.prepare_release(
            root=self.root,
            tag="v1.2.3",
            target="x86_64-unknown-linux-gnu",
            binary=self.binary,
            output_directory=self.root / "dist",
            mtime=123456789,
        )

        self.assertEqual(archive.name, "wlapse-v1.2.3-x86_64-unknown-linux-gnu.tar.xz")
        self.assertEqual(checksums.name, "SHA256SUMS")
        expected_prefix = "wlapse-v1.2.3-x86_64-unknown-linux-gnu"
        with tarfile.open(archive, "r:xz") as release_archive:
            members = {member.name: member for member in release_archive.getmembers()}
            self.assertEqual(
                set(members),
                {
                    expected_prefix,
                    f"{expected_prefix}/wlapse",
                    f"{expected_prefix}/README.md",
                    f"{expected_prefix}/LICENSE",
                },
            )
            self.assertEqual(
                stat.S_IMODE(members[f"{expected_prefix}/wlapse"].mode), 0o755
            )
            extracted_binary = release_archive.extractfile(f"{expected_prefix}/wlapse")
            assert extracted_binary is not None
            self.assertEqual(extracted_binary.read(), self.binary.read_bytes())
            self.assertTrue(
                all(member.mtime == 123456789 for member in members.values())
            )

        checksum, filename = checksums.read_text(encoding="ascii").strip().split("  ")
        self.assertEqual(filename, archive.name)
        self.assertEqual(checksum, prepare_release.sha256(archive))

        repeated_archive, repeated_checksums = prepare_release.prepare_release(
            root=self.root,
            tag="v1.2.3",
            target="x86_64-unknown-linux-gnu",
            binary=self.binary,
            output_directory=self.root / "repeated-dist",
            mtime=123456789,
        )
        self.assertEqual(repeated_archive.read_bytes(), archive.read_bytes())
        self.assertEqual(repeated_checksums.read_bytes(), checksums.read_bytes())


if __name__ == "__main__":
    unittest.main()
