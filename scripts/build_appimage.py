#!/usr/bin/env python3
"""Build the x86_64 wlapse AppImage."""

from __future__ import annotations

import argparse
import os
import re
import shutil
import stat
import subprocess
import tempfile
from pathlib import Path

_VERSION_PATTERN = re.compile(r"^[0-9A-Za-z.-]+$")
_APP_RUN = """#!/bin/sh
exec \"$APPDIR/usr/bin/wlapse\" \"$@\"
"""


def _require_file(path: Path, description: str) -> None:
    if not path.is_file():
        raise ValueError(f"{description} is missing: {path}")


def build(
    *,
    root: Path,
    version: str,
    binary: Path,
    appimagetool: Path,
    runtime: Path,
    output_directory: Path,
) -> Path:
    if not _VERSION_PATTERN.fullmatch(version):
        raise ValueError(f"invalid version: {version!r}")
    _require_file(binary, "release binary")
    if not os.access(binary, os.X_OK):
        raise ValueError(f"release binary is not executable: {binary}")
    _require_file(appimagetool, "appimagetool")
    if not os.access(appimagetool, os.X_OK):
        raise ValueError(f"appimagetool is not executable: {appimagetool}")
    _require_file(runtime, "AppImage runtime")

    desktop_file = root / "packaging/wlapse.desktop"
    icon_file = root / "packaging/wlapse.svg"
    _require_file(desktop_file, "desktop file")
    _require_file(icon_file, "application icon")

    output_directory.mkdir(parents=True, exist_ok=True)
    output = output_directory / f"wlapse-v{version}-x86_64.AppImage"

    with tempfile.TemporaryDirectory(prefix="wlapse-appimage-") as temporary_directory:
        appdir = Path(temporary_directory) / "wlapse.AppDir"
        binary_destination = appdir / "usr/bin/wlapse"
        icon_destination = (
            appdir / "usr/share/icons/hicolor/scalable/apps/wlapse.svg"
        )
        binary_destination.parent.mkdir(parents=True)
        icon_destination.parent.mkdir(parents=True)

        shutil.copyfile(binary, binary_destination)
        binary_destination.chmod(0o755)
        shutil.copyfile(desktop_file, appdir / "wlapse.desktop")
        (appdir / "wlapse.desktop").chmod(0o644)
        shutil.copyfile(icon_file, appdir / "wlapse.svg")
        (appdir / "wlapse.svg").chmod(0o644)
        shutil.copyfile(icon_file, icon_destination)
        icon_destination.chmod(0o644)
        (appdir / "AppRun").write_text(_APP_RUN, encoding="utf-8")
        (appdir / "AppRun").chmod(0o755)

        environment = os.environ.copy()
        environment.update({"APPIMAGE_EXTRACT_AND_RUN": "1", "ARCH": "x86_64"})
        subprocess.run(
            [appimagetool, "--runtime-file", runtime, appdir, output],
            check=True,
            env=environment,
        )

    _require_file(output, "AppImage output")
    output.chmod(output.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--appimagetool", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    arguments = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    output = build(
        root=root,
        version=arguments.version,
        binary=arguments.binary.resolve(),
        appimagetool=arguments.appimagetool.resolve(),
        runtime=arguments.runtime.resolve(),
        output_directory=arguments.output_dir.resolve(),
    )
    print(output)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        raise SystemExit(f"build_appimage.py: {error}") from error
