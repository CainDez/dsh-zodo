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
EVID = ROOT / "artifacts" / "evidence" / "a4"
EVID.mkdir(parents=True, exist_ok=True)


def start(name):
    data = ROOT / ".runtime" / f"dat04-{name}"
    if data.exists():
        shutil.rmtree(data)
    data.mkdir(parents=True)
    log = (EVID / f"dat04-{name}.log").open("w", encoding="utf-8")
    proc = subprocess.Popen([str(Z), "--data-dir", str(data), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)
    info = None
    for _ in range(150):
        time.sleep(0.1)
        path = data / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                if info.get("pid") == proc.pid:
                    break
            except Exception:
                pass
    if not info:
        raise RuntimeError(f"{name} did not start")
    return proc, log, info


def call(info, method, path, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
        headers={"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


source = target = None
log_source = log_target = None
try:
    source, log_source, source_info = start("source")
    call(source_info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": "dat04-source", "idempotency_key": f"dat04-{uuid.uuid4()}",
        "workspace_id": "ws_local", "operation": "task.create", "operation_version": 1,
        "payload": {"task": {"title": "批量导入来源任务", "estimate_minutes": 20}},
    })
    _, bundle = call(source_info, "GET", "/v1/data/export")

    target, log_target, target_info = start("target")
    preflight_status, preflight = call(target_info, "POST", "/v1/data/import/preflight", {
        "bundle": bundle,
    })
    first_status, first = call(target_info, "POST", "/v1/data/import", {
        "bundle": bundle, "idempotency_key": "dat04-key",
    })
    replay_status, replay = call(target_info, "POST", "/v1/data/import", {
        "bundle": bundle, "idempotency_key": "dat04-key",
    })
    modified = json.loads(json.dumps(bundle))
    modified["facts"]["tasks"][0]["title"] = "被篡改的导入"
    mismatch_status, mismatch = call(target_info, "POST", "/v1/data/import", {
        "bundle": modified, "idempotency_key": "dat04-key",
    })
    _, snapshot = call(target_info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": "dat04-q", "workspace_id": "ws_local",
        "query": "workspace.snapshot", "payload": {},
    })
    result = {
        "preflight_status": preflight_status,
        "preflight": preflight,
        "first_status": first_status,
        "first": first,
        "replay_status": replay_status,
        "replay": replay,
        "revision_after_first": first.get("revision"),
        "revision_after_replay": replay.get("revision"),
        "replay_flag": replay.get("replayed"),
        "mismatch_status": mismatch_status,
        "mismatch_code": mismatch.get("error", {}).get("code"),
        "tasks_after": len(snapshot["data"].get("tasks", [])),
    }
    (EVID / "a4-dat04-import.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "preflight_can_import": preflight.get("can_import"),
        "preflight_fingerprint": preflight.get("fingerprint"),
        "first_revision": result["revision_after_first"],
        "replay_replayed": result["replay_flag"],
        "replay_revision_same": result["revision_after_first"] == result["revision_after_replay"],
        "mismatch_status": mismatch_status,
        "mismatch_code": result["mismatch_code"],
        "tasks_after": result["tasks_after"],
    }, ensure_ascii=False, indent=2))
finally:
    for proc, log in ((source, log_source), (target, log_target)):
        if log:
            log.close()
        if proc:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except Exception:
                proc.kill()
