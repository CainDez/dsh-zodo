import json
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
EVID = ROOT / "artifacts" / "evidence" / "a4"
INFO = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
BASE = f"http://{INFO['host']}:{INFO['port']}"


def request(base, method, path, token, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


status, export = request(BASE, "GET", "/v1/data/export", INFO["tokens"]["ui"])
if status != 200:
    raise RuntimeError(export)
DATA = ROOT / ".runtime" / "a4-import-test"
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
log = (EVID / "a4-import-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen(
    [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
    stdout=log,
    stderr=subprocess.STDOUT,
)
try:
    info = None
    for _ in range(150):
        time.sleep(0.1)
        path = DATA / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                if info.get("pid") == proc.pid:
                    break
            except Exception:
                pass
    if not info:
        raise RuntimeError("fresh runtime did not start")
    fresh = f"http://{info['host']}:{info['port']}"
    status, imported = request(
        fresh,
        "POST",
        "/v1/data/import",
        info["tokens"]["ui"],
        {"bundle": export},
    )
    if status != 200:
        raise RuntimeError(imported)
    qs, snapshot = request(fresh, "POST", "/v1/queries", info["tokens"]["ui"], {
        "schema": "zodo.query/1",
        "request_id": "import-check",
        "workspace_id": "ws_local",
        "query": "workspace.snapshot",
        "payload": {},
    })
    data = snapshot["data"]
    result = {
        "export_counts": export.get("counts"),
        "import_status": status,
        "imported": imported,
        "fresh_counts": {
            "tasks": len(data.get("tasks", [])),
            "goals": len(data.get("goals", [])),
            "projects": len(data.get("projects", [])),
            "inbox": len(data.get("inbox", [])),
            "habits": len(data.get("habits", [])),
        },
        "revision": snapshot.get("revision"),
        "auth_file_absent": not (DATA / "auth.json").exists(),
    }
    (EVID / "a4-import.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
