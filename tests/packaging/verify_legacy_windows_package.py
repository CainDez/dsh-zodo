"""Verify Windows plugin layout at the pinned historical 0.2.0-dev.1 source."""
import hashlib
import json
from pathlib import Path
import runpy
import stat
import sys
from zipfile import ZipFile


ROOT = Path(__file__).resolve().parents[2]
CORE = "bin/win32-x64/zodo-core.exe"
OLD = "c3f3d0d63c7467dee14c241c2c05405dee7c2130"


def require(condition):
    if not condition:
        raise ValueError("Historical package verification failed")


def verify(source):
    release = source / "artifacts/release"
    plugin = source / "adapters/deepseek-harness/plugin"
    zip_path = release / "zodo-harness-plugin-0.2.0-dev.1.zip"
    build = json.loads((release / "BUILD_INFO.json").read_text(encoding="utf-8-sig"))
    require(build["commit"] == OLD and build["working_tree_dirty"] is False)
    manifest = json.loads((plugin / "package.json").read_bytes())
    require(manifest["name"] == "@zodo/harness-plugin" and manifest["version"] == "0.2.0-dev.1")
    expected = {"package.json", "index.js", "client.js", "privacy.js",
                "cordis.patch.yml", "icon.svg", "README.md", CORE}
    locale = {"locale/" + p.name for p in (plugin / "locale").iterdir() if p.is_file()}
    require(bool(locale))
    expected |= locale
    dirs = {"locale/", "bin/", "bin/win32-x64/"}
    executable = (release / "zodo.exe").read_bytes()
    require_x64 = runpy.run_path(str(ROOT / "scripts/verify-plugin-package.py"))["require_windows_x64"]
    require_x64(executable)
    with ZipFile(zip_path) as archive:
        entries = archive.infolist()
        names = {e.orig_filename for e in entries}
        require(len(names) == len(entries) and names <= expected | dirs)
        require({e.orig_filename for e in entries if not e.is_dir()} == expected)
        for entry in entries:
            require(stat.S_IFMT(entry.external_attr >> 16) != stat.S_IFLNK)
            if not entry.is_dir():
                name = entry.orig_filename
                data = executable if name == CORE else (plugin / name).read_bytes()
                require(entry.file_size == len(data) and archive.read(entry) == data)
    with zip_path.open("rb") as package:
        zip_hash = hashlib.file_digest(package, "sha256").hexdigest()
    return {
        "status": "passed",
        "historical_source_sha": OLD,
        "historical_plugin_version": "0.2.0-dev.1",
        "historical_plugin_sha256": zip_hash,
        "historical_core_sha256": hashlib.sha256(executable).hexdigest(),
        "windows_x64_package": True,
    }


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError()
        print(json.dumps(verify(Path(sys.argv[1]).resolve(strict=True))))
    except Exception:
        print('{"status":"failed","windows_x64_package":false}')
        raise SystemExit(1)
