"""Validate a wlapse release and build deterministic release assets."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import tarfile
import tempfile
from pathlib import Path

_NUMERIC = r"(?:0|[1-9][0-9]*)"
_PRERELEASE_IDENTIFIER = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
_TAG_PATTERN = re.compile(
    rf"^v({_NUMERIC}\.{_NUMERIC}\.{_NUMERIC}(?:-{_PRERELEASE_IDENTIFIER}(?:\.{_PRERELEASE_IDENTIFIER})*)?)$"
)
_TARGET_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+$")


def parse_tag_version(tag: str) -> str:
    match = _TAG_PATTERN.fullmatch(tag)
    if match is None:
        raise ValueError(
            f"{tag!r} is not a valid release tag; expected vMAJOR.MINOR.PATCH "
            "with an optional SemVer prerelease suffix"
        )
    return match.group(1)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _manifest_version(root: Path) -> str:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            root / "Cargo.toml",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    versions = [
        package["version"]
        for package in metadata.get("packages", [])
        if package.get("name") == "wlapse"
    ]
    if len(versions) != 1 or not isinstance(versions[0], str):
        raise ValueError("Cargo.toml does not define exactly one wlapse package")
    return versions[0]


def _binary_version(binary: Path) -> str:
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"release binary is missing or not executable: {binary}")
    result = subprocess.run(
        [binary, "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def _tar_info(name: str, mode: int, mtime: int, size: int = 0) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.mode = mode
    info.mtime = mtime
    info.uid = 0
    info.gid = 0
    info.uname = "root"
    info.gname = "root"
    info.size = size
    return info


def _add_file(
    archive: tarfile.TarFile,
    source: Path,
    archive_name: str,
    mode: int,
    mtime: int,
) -> None:
    if not source.is_file():
        raise ValueError(f"required release file is missing: {source}")
    data = source.read_bytes()
    archive.addfile(
        _tar_info(archive_name, mode, mtime, len(data)), fileobj=io.BytesIO(data)
    )


def prepare_release(
    *,
    root: Path,
    tag: str,
    target: str,
    binary: Path,
    output_directory: Path,
    mtime: int = 0,
) -> tuple[Path, Path]:
    version = parse_tag_version(tag)
    manifest_version = _manifest_version(root)
    if version != manifest_version:
        raise ValueError(
            f"tag version {version!r} does not match Cargo.toml version "
            f"{manifest_version!r}"
        )
    if not _TARGET_PATTERN.fullmatch(target):
        raise ValueError(f"invalid target name: {target!r}")

    reported_version = _binary_version(binary)
    expected_version = f"wlapse {version}"
    if reported_version != expected_version:
        raise ValueError(
            f"binary version {reported_version!r} does not match expected "
            f"version {expected_version!r}"
        )

    output_directory.mkdir(parents=True, exist_ok=True)
    base_name = f"wlapse-{tag}-{target}"
    archive_path = output_directory / f"{base_name}.tar.xz"
    checksum_path = output_directory / "SHA256SUMS"

    with tempfile.NamedTemporaryFile(
        dir=output_directory, prefix=f".{base_name}.", suffix=".tar.xz", delete=False
    ) as temporary:
        temporary_archive = Path(temporary.name)
    try:
        with tarfile.open(
            temporary_archive, "w:xz", format=tarfile.PAX_FORMAT
        ) as archive:
            directory = _tar_info(base_name, 0o755, mtime)
            directory.type = tarfile.DIRTYPE
            archive.addfile(directory)
            _add_file(archive, binary, f"{base_name}/wlapse", 0o755, mtime)
            _add_file(
                archive, root / "README.md", f"{base_name}/README.md", 0o644, mtime
            )
            _add_file(archive, root / "LICENSE", f"{base_name}/LICENSE", 0o644, mtime)
        temporary_archive.replace(archive_path)
    finally:
        temporary_archive.unlink(missing_ok=True)

    checksum_path.write_text(
        f"{sha256(archive_path)}  {archive_path.name}\n", encoding="ascii"
    )
    return archive_path, checksum_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    parser.add_argument(
        "--mtime",
        type=int,
        default=int(os.environ.get("SOURCE_DATE_EPOCH", "0")),
    )
    arguments = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    archive, checksums = prepare_release(
        root=root,
        tag=arguments.tag,
        target=arguments.target,
        binary=arguments.binary.resolve(),
        output_directory=arguments.output_dir.resolve(),
        mtime=arguments.mtime,
    )
    print(archive)
    print(checksums)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.CalledProcessError, TypeError, ValueError) as error:
        raise SystemExit(f"prepare_release.py: {error}") from error
