#!/usr/bin/env python3
"""Check a Zodo Windows plugin ZIP against an exact source checkout and build."""
import argparse
import hashlib
import json
import stat
import struct
from pathlib import Path
from zipfile import BadZipFile, ZipFile

CORE = "bin/win32-x64/zodo-core.exe"
SOURCE = Path(__file__).resolve().parents[1] / "adapters/deepseek-harness/plugin"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def source_files(source):
    manifest = json.loads((source / "package.json").read_bytes())
    expected = {"package.json", "README.md"}
    for pattern in manifest["files"]:
        if pattern == "locale/*.json":
            locales = sorted((source / "locale").glob("*.json"))
            require(bool(locales), "No locale JSON resources in source")
            expected.update(path.relative_to(source).as_posix() for path in locales)
        else:
            require(pattern in {
                "index.js", "client.js", "privacy.js", "host-gateway.js",
                "cordis.patch.yml", "icon.svg", CORE,
            }, "Unexpected package manifest file; update the verification contract")
            expected.add(pattern)
    required = {"index.js", "client.js", "privacy.js", "host-gateway.js",
                "cordis.patch.yml", "icon.svg", CORE}
    require(required <= expected, "Package manifest omits required files")
    for target in [*manifest["exports"].values(), manifest["dsh"]["bundle"]["patch"]]:
        require(target.startswith("./") and
                (target[2:] in expected or target == "./locale/*.json"),
                "Manifest export or patch points outside the package")
    return expected


def require_windows_x64(binary):
    require(len(binary) >= 64 and binary[:2] == b"MZ", "Core is not a PE executable")
    offset = struct.unpack_from("<I", binary, 60)[0]
    require(64 <= offset <= len(binary) - 26 and binary[offset:offset + 4] == b"PE\0\0",
            "Invalid Core PE header")
    machine, = struct.unpack_from("<H", binary, offset + 4)
    options, flags, magic = struct.unpack_from("<HHH", binary, offset + 20)
    require(machine == 0x8664 and magic == 0x20B and
            2 <= options <= len(binary) - offset - 24 and flags & 2 and not flags & 0x2000,
            "Core is not a Windows x64 executable")


def verify(package, core_path, source=SOURCE):
    expected = source_files(source)
    core = core_path.read_bytes()
    require_windows_x64(core)
    with ZipFile(package) as archive:
        records = archive.infolist()
        names = {entry.orig_filename for entry in records}
        require(len(names) == len(records), "Duplicate ZIP entries")
        dirs = {part.as_posix() + "/" for name in expected
                for part in Path(name).parents if part != Path(".")}
        require({entry.orig_filename for entry in records if not entry.is_dir()} == expected
                and names <= expected | dirs, "Extra or missing ZIP entries")
        for entry in records:
            require(stat.S_IFMT(entry.external_attr >> 16) != stat.S_IFLNK,
                    "Unexpected ZIP symbolic link")
            if entry.is_dir():
                continue
            name = entry.orig_filename
            expected_bytes = core if name == CORE else (source / name).read_bytes()
            require(entry.file_size == len(expected_bytes) and archive.read(entry) == expected_bytes,
                    f"ZIP bytes differ from the verified input: {name}")
    return hashlib.sha256(core).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("core", type=Path, help="zodo.exe from the same build")
    args = parser.parse_args()
    try:
        core_hash = verify(args.package, args.core)
    except (BadZipFile, OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        parser.exit(1, f"Package verification failed: {error}\n")
    print(f"Package contents match checkout and Windows x64 Core SHA-256 {core_hash}")


if __name__ == "__main__":
    main()
