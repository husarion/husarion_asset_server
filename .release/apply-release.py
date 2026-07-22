#!/usr/bin/env python3
"""Apply a release: bump the [package] version in Cargo.toml, the matching entry
in Cargo.lock, and the ament <version> in package.xml, and prepend the new
section to CHANGELOG.md.

Called from the `release` recipe in the justfile after the operator confirms the
proposed version. Pure Python, stdlib-only — no cargo needed (the host has no
ROS to build this r2r crate; CI builds the release binaries on tag push).

Usage:
    apply-release.py <version> <section_file> <cargo_toml> <cargo_lock> <package_xml> <changelog>
"""

from __future__ import annotations

import datetime
import pathlib
import re
import sys

CRATE = "husarion_asset_server"


def bump_package_version(path: pathlib.Path, version: str) -> None:
    """Cargo.toml — the `version = "X.Y.Z"` under `[package]`."""
    text = path.read_text()
    pattern = re.compile(r'(\[package\][^\[]*?\nversion\s*=\s*)"[^"]+"', re.DOTALL)
    new, n = pattern.subn(rf'\1"{version}"', text, count=1)
    if n == 0:
        sys.exit(f"apply-release: no [package] version found in {path}")
    path.write_text(new)


def bump_lock_version(path: pathlib.Path, version: str) -> None:
    """Cargo.lock — the `version` line in the `[[package]] name = "<CRATE>"`
    stanza. Keeps a `--locked` consumer build consistent without running cargo."""
    if not path.exists():
        return
    text = path.read_text()
    pattern = re.compile(
        r'(\[\[package\]\]\nname = "' + re.escape(CRATE) + r'"\nversion = )"[^"]+"'
    )
    new, n = pattern.subn(rf'\1"{version}"', text, count=1)
    if n == 0:
        sys.exit(f"apply-release: no [[package]] {CRATE} entry found in {path}")
    path.write_text(new)


def bump_package_xml(path: pathlib.Path, version: str) -> None:
    """package.xml — the ament `<version>X.Y.Z</version>`. Hand-maintained and
    kept in lockstep with Cargo.toml so `ros2 pkg` / colcon report the real
    version (the drift the bridge hit at 0.8.1)."""
    if not path.exists():
        return
    text = path.read_text()
    new, n = re.subn(r"<version>[^<]+</version>", f"<version>{version}</version>", text, count=1)
    if n == 0:
        sys.exit(f"apply-release: no <version> found in {path}")
    path.write_text(new)


def prepend_changelog(changelog: pathlib.Path, version: str, section: str) -> None:
    today = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
    block = f"## [{version}] — {today}\n\n{section.rstrip()}\n\n"
    text = changelog.read_text() if changelog.exists() else "# Changelog\n\n"
    m = re.search(r"(?m)^## \[", text)
    if m:
        text = text[: m.start()] + block + text[m.start():]
    else:
        text = text.rstrip() + "\n\n" + block
    changelog.write_text(text)


def main(argv: list[str]) -> int:
    if len(argv) != 7:
        sys.exit(
            f"usage: {argv[0]} <version> <section_file> <cargo_toml> <cargo_lock> <package_xml> <changelog>"
        )
    _, version, section_path, cargo_path, lock_path, pkgxml_path, changelog_path = argv
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        sys.exit(f"apply-release: invalid semver '{version}'")
    section = pathlib.Path(section_path).read_text()
    bump_package_version(pathlib.Path(cargo_path), version)
    bump_lock_version(pathlib.Path(lock_path), version)
    bump_package_xml(pathlib.Path(pkgxml_path), version)
    prepend_changelog(pathlib.Path(changelog_path), version, section)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
