#!/usr/bin/env python3
"""List or extract files from an Electron asar archive without npm dependencies.

This is kept in the repository so A0 evidence can be reproduced against the
installed DeepSeek Harness application package.
"""
from __future__ import annotations

import argparse
import json
import os
import struct
import sys
from pathlib import Path


def read_header(archive: Path) -> tuple[dict, int]:
    with archive.open("rb") as handle:
        prefix = handle.read(16)
        if len(prefix) < 16:
            raise ValueError("not an asar archive: short header")
        header_size = struct.unpack("<I", prefix[4:8])[0]
        handle.seek(16)
        raw = handle.read(header_size)
    text = raw.decode("utf-8", errors="strict")
    header, end = json.JSONDecoder().raw_decode(text)
    header_bytes = len(text[:end].encode("utf-8"))
    data_start = 16 + header_bytes
    data_start = (data_start + 3) // 4 * 4
    return header, data_start


def walk(node: dict, path: str = ""):
    for name, value in node.get("files", {}).items():
        current = f"{path}/{name}" if path else name
        if "files" in value:
            yield from walk(value, current)
        else:
            yield current, value


def find(header: dict, pattern: str) -> list[tuple[str, dict]]:
    needle = pattern.lower()
    return [(path, value) for path, value in walk(header) if needle in path.lower()]


def extract_one(archive: Path, data_start: int, path: str, value: dict, out: Path) -> Path:
    if value.get("unpacked"):
        raise ValueError(f"{path} is stored unpacked next to the archive")
    size = int(value.get("size", 0))
    offset = int(value.get("offset", 0))
    target = out / path
    target.parent.mkdir(parents=True, exist_ok=True)
    with archive.open("rb") as handle:
        handle.seek(data_start + offset)
        target.write_bytes(handle.read(size))
    return target


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--list", metavar="SUBSTRING", help="list paths containing this text")
    parser.add_argument("--extract", metavar="SUBSTRING", help="extract paths containing this text")
    parser.add_argument("--out", type=Path, help="destination directory for --extract")
    parser.add_argument("--exact", action="store_true", help="treat pattern as exact path")
    args = parser.parse_args(argv)

    header, data_start = read_header(args.archive)
    entries = list(walk(header))
    selected = [
        (path, value)
        for path, value in entries
        if (path == args.list or path == args.extract) if args.exact
    ] if args.exact else [
        (path, value)
        for path, value in entries
        if (args.list and args.list.lower() in path.lower())
        or (args.extract and args.extract.lower() in path.lower())
    ]

    if args.list:
        for path, value in selected:
            print(f"{value.get('size', 0):>10}  {'U' if value.get('unpacked') else '-'}  {path}")
        print(f"# {len(selected)} matching entries / {len(entries)} total", file=sys.stderr)
        return 0

    if args.extract:
        if not args.out:
            parser.error("--extract requires --out")
        if not selected:
            print(f"no entries matched {args.extract!r}", file=sys.stderr)
            return 2
        for path, value in selected:
            if value.get("unpacked"):
                print(f"skip unpacked: {path}", file=sys.stderr)
                continue
            extracted = extract_one(args.archive, data_start, path, value, args.out)
            print(extracted)
        return 0

    parser.print_help()
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
