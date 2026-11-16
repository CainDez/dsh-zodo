import json
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "h09-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
log = (EVID / "h09-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)


def call(info, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data,
        method="POST" if body is not None else "GET",
        headers={"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def envelope(key, title):
    return {
        "schema": "zodo.command/1", "request_id": "h09-request", "idempotency_key": key,
        "workspace_id": "ws_local", "operation": "task.create", "operation_version": 1,
        "payload": {"task": {"title": title, "estimate_minutes": 10}},
    }


def tasks(info):
    status, body = call(info, "/v1/queries", {
        "schema": "zodo.query/1", "request_id": "h09-q", "workspace_id": "ws_local",
        "query": "tasks.list", "payload": {"filter": {"include_archived": True}},
    })
    return body["data"]["tasks"]


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
    first_status, first = call(info, "/v1/commands", envelope("h09-key", "回执丢失测试"))
    count_after_first = len(tasks(info))
    replay_status, replay = call(info, "/v1/commands", envelope("h09-key", "回执丢失测试"))
    count_after_replay = len(tasks(info))
    mismatch_status, mismatch = call(info, "/v1/commands", envelope("h09-key", "不同的内容"))
    count_after_mismatch = len(tasks(info))
    result = {
        "first_status": first_status,
        "first_replayed": first.get("replayed"),
        "first_revision": first.get("revision"),
        "count_after_first": count_after_first,
        "replay_status": replay_status,
        "replay_replayed": replay.get("replayed"),
        "replay_revision": replay.get("revision"),
        "count_after_replay": count_after_replay,
        "mismatch_status": mismatch_status,
        "mismatch_code": mismatch.get("error", {}).get("code"),
        "count_after_mismatch": count_after_mismatch,
    }
    (EVID / "h09-receipt-replay.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
