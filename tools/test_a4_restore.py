import json
import os
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "a4-restore-test"
EVID = ROOT / "artifacts" / "evidence" / "a4"
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
env = os.environ.copy()
env["OLLAMA_API_KEY"] = "ollama"
def start(log):
    p = subprocess.Popen(
        [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
        stdout=log,
        stderr=subprocess.STDOUT,
        env=env,
    )
    info = None
    for _ in range(150):
        time.sleep(0.1)
        path = DATA / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                if info.get("pid") == p.pid:
                    return p, info
            except Exception:
                pass
    p.kill()
    raise RuntimeError("runtime did not start")


def call(info, method, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path,
        data=data,
        method=method,
        headers={"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def command(info, operation, payload):
    return call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1",
        "request_id": f"restore-{uuid.uuid4()}",
        "idempotency_key": f"restore-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "operation": operation,
        "operation_version": 1,
        "payload": payload,
    })


def query(info, name, payload):
    return call(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1",
        "request_id": f"restore-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": name,
        "payload": payload,
    })


log1 = (EVID / "a4-restore-server-1.log").open("w", encoding="utf-8")
proc, info = start(log1)
try:
    command(info, "task.create", {"task": {"title": "备份前任务", "estimate_minutes": 10}})
    status, backup = call(info, "POST", "/v1/data/backup", {})
    backup_path = backup.get("path")
    command(info, "task.create", {"task": {"title": "备份后任务", "estimate_minutes": 10}})
    status, before = query(info, "tasks.list", {"filter": {"limit": 100}})
    before_count = len(before["data"]["tasks"])
    proc.terminate()
    proc.wait(timeout=10)
    log1.close()
    restore = subprocess.run(
        [str(Z), "--data-dir", str(DATA), "restore", backup_path],
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    restored = json.loads(restore.stdout) if restore.stdout.strip().startswith("{") else {"stdout": restore.stdout, "stderr": restore.stderr}
    log2 = (EVID / "a4-restore-server-2.log").open("w", encoding="utf-8")
    proc2, info2 = start(log2)
    try:
        status, after = query(info2, "tasks.list", {"filter": {"limit": 100}})
        after_count = len(after["data"]["tasks"])
        runtime_exists = (DATA / "runtime.json").exists()
        auth_exists = (DATA / "auth.json").exists()
        pre_restore = list((DATA / "backups").glob("pre-restore-*.sqlite3"))
        result = {
            "backup": backup,
            "before_count": before_count,
            "restore_exit": restore.returncode,
            "restore_stdout": restore.stdout,
            "restore_stderr": restore.stderr,
            "after_count": after_count,
            "auth_file_exists": auth_exists,
            "runtime_json_exists": runtime_exists,
            "pre_restore_snapshots": [str(path) for path in pre_restore],
            "query_status": status,
        }
        (EVID / "a4-restore.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps({
            "before_count": before_count,
            "restore_exit": restore.returncode,
            "after_count": after_count,
            "pre_restore": len(pre_restore),
            "auth_reset": not auth_exists,
            "status": status,
        }, ensure_ascii=False, indent=2))
    finally:
        proc2.terminate()
        try:
            proc2.wait(timeout=5)
        except Exception:
            proc2.kill()
        log2.close()
finally:
    if proc.poll() is None:
        proc.kill()
