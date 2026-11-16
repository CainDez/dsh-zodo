import json
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "fnd11-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
log = (EVID / "fnd11-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)


def request(info, method, path, body=None):
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


def query(info, payload):
    return request(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": f"ev-{uuid.uuid4()}", "workspace_id": "ws_local",
        "query": "events.list", "payload": payload,
    })


def command(info, payload):
    return request(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": f"ev-{uuid.uuid4()}", "idempotency_key": f"ev-{uuid.uuid4()}",
        "workspace_id": "ws_local", "operation": "task.create", "operation_version": 1, "payload": payload,
    })


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
        raise RuntimeError("Core did not start")
    command(info, {"task": {"title": "事件一", "estimate_minutes": 5}})
    command(info, {"task": {"title": "事件二", "estimate_minutes": 5}})
    first = query(info, {"limit": 100})
    first_data = first[1]["data"]
    cursor = first_data["next_cursor"]
    command(info, {"task": {"title": "事件三", "estimate_minutes": 5}})
    second = query(info, {"after_cursor": cursor, "limit": 100})
    second_data = second[1]["data"]
    duplicate = query(info, {"after_cursor": second_data["next_cursor"], "limit": 100})
    future = query(info, {"after_cursor": "ev_999999", "limit": 100})
    limited = query(info, {"after_cursor": "ev_0", "limit": 1})
    result = {
        "first_count": len(first_data["events"]),
        "first_next_cursor": cursor,
        "second_count": len(second_data["events"]),
        "second_types": [event["event_type"] for event in second_data["events"]],
        "duplicate_count": len(duplicate[1]["data"]["events"]),
        "future_resync_required": future[1]["data"]["resync_required"],
        "limited_has_more": limited[1]["data"]["has_more"],
        "limited_next_cursor": limited[1]["data"]["next_cursor"],
        "all_queries_ok": all(status == 200 for status, _ in (first, second, duplicate, future, limited)),
    }
    (EVID / "fnd11-events.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
