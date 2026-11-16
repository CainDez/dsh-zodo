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
DATA = ROOT / ".runtime" / "h10-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)


def start():
    log = (EVID / f"h10-server-{int(time.time())}.log").open("w", encoding="utf-8")
    proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)
    info = None
    for _ in range(150):
        time.sleep(0.1)
        path = DATA / "runtime.json"
        if path.exists():
            try:
                value = json.loads(path.read_text(encoding="utf-8"))
                if value.get("pid") == proc.pid:
                    info = value
                    break
            except Exception:
                pass
    if not info:
        raise RuntimeError("Core did not start")
    return proc, log, info


def call(info, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
        headers={"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def command(info, title):
    return call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": f"h10-{uuid.uuid4()}", "idempotency_key": f"h10-{uuid.uuid4()}",
        "workspace_id": "ws_local", "operation": "task.create", "operation_version": 1,
        "payload": {"task": {"title": title, "estimate_minutes": 10}},
    })


def events(info, cursor):
    return call(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": "h10-q", "workspace_id": "ws_local",
        "query": "events.list", "payload": {"after_cursor": cursor, "limit": 100},
    })


pid1 = pid2 = log1 = log2 = None
try:
    proc1, log1, info1 = start()
    command(info1, "重启前的任务")
    _, page = events(info1, "ev_0")
    cursor = page["data"]["next_cursor"]
    proc1.terminate()
    proc1.wait(timeout=5)
    log1.close()
    proc1 = None

    proc2, log2, info2 = start()
    command(info2, "重启后的任务")
    status, page2 = events(info2, cursor)
    result = {
        "first_cursor": cursor,
        "restart_status": status,
        "new_events": len(page2["data"]["events"]),
        "new_event_types": [event["event_type"] for event in page2["data"]["events"]],
        "resync_required": page2["data"]["resync_required"],
        "next_cursor": page2["data"]["next_cursor"],
        "same_workspace": info1["workspace_id"] == info2["workspace_id"],
        "same_instance": info1["instance_id"] == info2["instance_id"],
    }
    (EVID / "h10-restart-cursor.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    for proc, log in ((proc1, log1), (proc2, log2)):
        if log:
            log.close()
        if proc:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except Exception:
                proc.kill()
