import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "build_appimage.py"
SPEC = importlib.util.spec_from_file_location("build_appimage", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT}")
build_appimage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(build_appimage)


class AppImageBuildTests(unittest.TestCase):
    def test_builds_appimage_from_binary_and_packaging_files(self):
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            packaging = root / "packaging"
            packaging.mkdir()
            (packaging / "wlapse.desktop").write_text(
                "[Desktop Entry]\nType=Application\nName=wlapse\nExec=wlapse\nIcon=wlapse\n",
                encoding="utf-8",
            )
            (packaging / "wlapse.svg").write_text("<svg/>\n", encoding="utf-8")

            binary = root / "wlapse"
            binary.write_text("#!/bin/sh\n", encoding="utf-8")
            binary.chmod(0o755)
            runtime = root / "runtime-x86_64"
            runtime.write_bytes(b"runtime\n")
            tool = root / "appimagetool"
            tool.write_text(
                "#!/bin/sh\n"
                "set -eu\n"
                "test \"$1\" = --runtime-file\n"
                "test -f \"$2\"\n"
                "appdir=$3\n"
                "output=$4\n"
                "test -x \"$appdir/AppRun\"\n"
                "test -x \"$appdir/usr/bin/wlapse\"\n"
                "test -f \"$appdir/wlapse.desktop\"\n"
                "test -f \"$appdir/wlapse.svg\"\n"
                "test -f \"$appdir/usr/share/icons/hicolor/scalable/apps/wlapse.svg\"\n"
                "printf 'mock AppImage\\n' >\"$output\"\n",
                encoding="utf-8",
            )
            tool.chmod(0o755)

            output = build_appimage.build(
                root=root,
                version="1.2.3",
                binary=binary,
                appimagetool=tool,
                runtime=runtime,
                output_directory=root / "dist",
            )

            self.assertEqual(output.name, "wlapse-v1.2.3-x86_64.AppImage")
            self.assertEqual(output.read_bytes(), b"mock AppImage\n")
            self.assertTrue(output.stat().st_mode & 0o111)


if __name__ == "__main__":
    unittest.main()
