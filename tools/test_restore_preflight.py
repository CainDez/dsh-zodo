import hashlib
import json
import pathlib
import shutil
import subprocess

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
BACKUPS = sorted((ROOT / ".runtime" / "zodo-a2" / "backups").glob("*.sqlite3"))
EVID = ROOT / "artifacts" / "evidence" / "a6"
EVID.mkdir(parents=True, exist_ok=True)
DATA = ROOT / ".runtime" / "restore-preflight-test"
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
results = {}


def run(args):
    proc = subprocess.run([str(Z), "--data-dir", str(DATA), *args], capture_output=True, text=True)
    return {"args": args, "exit": proc.returncode, "stdout": proc.stdout.strip()[:2000], "stderr": proc.stderr.strip()[:2000]}


def digest():
    path = DATA / "zodo.sqlite3"
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


# Restore a known-good backup into a fresh directory.
first = run(["restore", str(BACKUPS[-1])])
results["first_restore"] = first
results["first_opt"] = json.loads(first["stdout"]) if first["exit"] == 0 and first["stdout"].startswith("{") else None
hash_after_first = digest()

# Restore a second backup over the live DB: a pre-restore snapshot must be written first.
second = run(["restore", str(BACKUPS[-2])])
results["second_restore"] = second
results["second_opt"] = json.loads(second["stdout"]) if second["exit"] == 0 and second["stdout"].startswith("{") else None
snapshots = sorted((DATA / "backups").glob("pre-restore-*.sqlite3")) if (DATA / "backups").exists() else []
results["pre_restore_snapshot_count"] = len(snapshots)

# Restoring the same backup twice is supported and still leaves a fresh pre-restore snapshot.
third = run(["restore", str(BACKUPS[-2])])
results["duplicate_restore"] = third
snapshots = sorted((DATA / "backups").glob("pre-restore-*.sqlite3"))
results["pre_restore_snapshot_count_after_duplicate"] = len(snapshots)

# A corrupted backup must fail closed and leave the live DB unchanged.
corrupt = DATA / "corrupt.sqlite3"
corrupt.write_text("this is not a sqlite database", encoding="utf-8")
hash_before_corrupt = digest()
corrupt_attempt = run(["restore", str(corrupt)])
results["corrupt_restore"] = corrupt_attempt
results["db_unchanged_after_corrupt"] = digest() == hash_before_corrupt

# A missing backup path must also fail closed.
missing = run(["restore", str(DATA / "missing.sqlite3")])
results["missing_restore"] = missing
(EVID / "restore-preflight.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({
    "first_exit": first["exit"],
    "first_auth_reset": (results["first_opt"] or {}).get("auth_reset"),
    "second_exit": second["exit"],
    "pre_restore_snapshot_count": len(snapshots),
    "corrupt_exit": corrupt_attempt["exit"],
    "corrupt_failed": corrupt_attempt["exit"] != 0,
    "db_unchanged_after_corrupt": results["db_unchanged_after_corrupt"],
    "missing_exit": missing["exit"],
}, ensure_ascii=False, indent=2))
