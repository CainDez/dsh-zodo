"""Fail closed on unreviewed DSH or pnpm versions, sources and transitive lock changes."""
import base64
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
PINS = {"@deepseek-ai/dsh": "0.2.2-rc.5", "pnpm": "11.25.0"}
LOCK_SHA256 = "372fc8c2bb735022e6111cd6c9c2930aed66e3710934487fe70077eb3bb1a828"


def check(condition):
    if not condition:
        raise SystemExit("Official Web dependency lock differs from the reviewed registry-only graph.")


raw = (HERE / "package-lock.json").read_bytes()
manifest = json.loads((HERE / "package.json").read_text())
lock = json.loads(raw)
name = "zodo-official-web-ci-deps"
check(manifest == {"name": name, "private": True, "dependencies": PINS})
check(lock["name"] == name and lock["lockfileVersion"] == 3)
packages = lock["packages"]
check(len(packages) == 745 and packages[""] == {"dependencies": PINS})
for dep, version in PINS.items():
    check(packages["node_modules/" + dep]["version"] == version)
package_pattern = r"(?:@[a-z0-9_.-]+/)?[a-z0-9_.-]+"
for key, entry in packages.items():
    if not key:
        continue
    path = key.split("node_modules/")[1:]
    check(bool(path) and all(re.fullmatch(package_pattern, part.rstrip("/")) for part in path))
    pkg = path[-1]
    version = entry["version"]
    check(bool(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.+-]+)?", version)))
    check(entry["resolved"] == f"https://registry.npmjs.org/{pkg}/-/{pkg.split('/')[-1]}-{version}.tgz")
    sri = entry["integrity"]
    check(sri.startswith("sha512-") and len(base64.b64decode(sri[7:], validate=True)) == 64)
    check(not entry.get("link") and not entry.get("inBundle"))
check(hashlib.sha256(raw).hexdigest() == LOCK_SHA256)
print("verified official Web registry pins: DSH 0.2.2-rc.5, pnpm 11.25.0, 744 tarballs")
