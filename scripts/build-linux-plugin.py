#!/usr/bin/env python3
"""Build and verify a Linux x64 Zodo plugin bundle in a new output directory."""
import argparse
import hashlib
import importlib.util
import json
import platform
from pathlib import Path
import stat
import subprocess
import sys
from tempfile import TemporaryDirectory
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
TARGET = "linux-x64"
RUST_TARGET = "x86_64-unknown-linux-gnu"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def assemble(archive, core, verifier):
    source = verifier.SOURCE
    with ZipFile(archive, "x", compression=ZIP_DEFLATED) as plugin:
        for name in sorted(verifier.source_files(source, TARGET)):
            item = ZipInfo(name)
            item.create_system = 3
            is_core = name == verifier.LINUX_CORE
            item.external_attr = (stat.S_IFREG | (0o755 if is_core else 0o644)) << 16
            plugin.writestr(item, (core if is_core else source / name).read_bytes(),
                            compress_type=ZIP_DEFLATED)
    return verifier.verify(archive, core, target=TARGET)


def build(output, offline):
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise ValueError("Build requires a native Linux x86_64 host and GNU Rust target")
    if output.exists() or output.is_symlink():
        raise ValueError("Use a new output directory; existing output is never replaced")

    spec = importlib.util.spec_from_file_location("verify_plugin_package",
                                                  ROOT / "scripts/verify-plugin-package.py")
    verifier = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(verifier)
    source = verifier.SOURCE
    ver = json.loads((source / "package.json").read_bytes())["version"]
    # Use a fixed archive name; the plugin's version is recorded in metadata.
    archive_name = "zodo-harness-plugin-linux-x64.zip"
    commit = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain", "--untracked-files=normal"))
    with TemporaryDirectory(prefix="zodo-linux-build-") as tmp:
        scratch = Path(tmp)
        cargo_target = scratch / "target"
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "zodo-runtime",
                        "--target", RUST_TARGET, "--target-dir", str(cargo_target),
                        *(["--offline"] if offline else [])], cwd=ROOT, check=True)
        core = cargo_target / RUST_TARGET / "release" / "zodo"
        archive = scratch / archive_name
        core_hash = assemble(archive, core, verifier)
        if git("rev-parse", "HEAD") != commit:
            raise ValueError("Source HEAD moved during the build; retry in a stable checkout")
        metadata = {
            "schema": "zodo.linux-plugin-build/1",
            "source_commit": commit,
            "working_tree_dirty_at_start": dirty,
            "plugin_version": ver,
            "cargo_target": RUST_TARGET,
            "core_sha256": core_hash,
            "plugin_zip_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        }
        output.mkdir()
        for name, payload in [(archive_name, archive.read_bytes()), ("zodo", core.read_bytes()),
                              ("BUILD_INFO.json", (json.dumps(metadata, indent=2) + "\n").encode())]:
            with (output / name).open("xb") as f:
                f.write(payload)
        # Keep the reference executable usable without modifying existing files.
        (output / "zodo").chmod(0o755)
    print(f"Verified {output / archive_name}; bundled Linux x64 Core SHA-256 {core_hash}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new output directory with an existing parent")
    parser.add_argument("--offline", action="store_true", help="pass --offline to Cargo")
    args = parser.parse_args()
    try:
        build(args.output, args.offline)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Linux plugin build failed: {error}\n")


if __name__ == "__main__":
    main()
