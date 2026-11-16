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
EVID.mkdir(parents=True, exist_ok=True)
procs = []
logs = []


def start(name):
    data = ROOT / ".runtime" / f"batch-{name}"
    if data.exists():
        shutil.rmtree(data)
    data.mkdir(parents=True)
    log = (EVID / f"batch-{name}.log").open("w", encoding="utf-8")
    proc = subprocess.Popen([str(Z), "--data-dir", str(data), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)
    procs.append(proc); logs.append(log)
    info = None
    for _ in range(200):
        time.sleep(0.1)
        path = data / "runtime.json"
        if path.exists():
            try:
                candidate = json.loads(path.read_text(encoding="utf-8"))
                if candidate.get("pid") == proc.pid:
                    info = candidate; break
            except Exception:
                pass
    if not info:
        raise RuntimeError(f"core {name} did not start")
    return info


def request(info, method, path, token, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode()
    req = urllib.request.Request(f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
                                 headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def command(info, operation, payload):
    return request(info, "POST", "/v1/commands", info["tokens"]["ui"], {
        "schema": "zodo.command/1", "request_id": f"batch-{time.time_ns()}",
        "idempotency_key": f"batch-{time.time_ns()}", "workspace_id": "ws_local",
        "operation": operation, "operation_version": 1, "payload": payload,
    })


def tasks(info):
    _, body = request(info, "POST", "/v1/queries", info["tokens"]["ui"], {
        "schema": "zodo.query/1", "request_id": "batch-list", "workspace_id": "ws_local",
        "query": "tasks.list", "payload": {"filter": {"limit": 100}},
    })
    return body["data"]["tasks"]


try:
    a = start("a"); b = start("b"); c = start("c")
    command(a, "task.create", {"task": {"title": "批次 A", "estimate_minutes": 10}})
    command(b, "task.create", {"task": {"title": "批次 B", "estimate_minutes": 10}})
    _, bundle_a = request(a, "GET", "/v1/data/export", a["tokens"]["ui"])
    _, bundle_b = request(b, "GET", "/v1/data/export", b["tokens"]["ui"])
    status_batch, batch = request(c, "POST", "/v1/data/import", c["tokens"]["ui"], {
        "bundles": [bundle_a, bundle_b], "idempotency_key": "batch-key",
    })
    c_tasks = tasks(c)
    status_replay, replay = request(c, "POST", "/v1/data/import", c["tokens"]["ui"], {
        "bundles": [bundle_a, bundle_b], "idempotency_key": "batch-key",
    })
    # Conflicting bundles are rejected before any write (new target core).
    d = start("d")
    modified = json.loads(json.dumps(bundle_a))
    for task in modified["facts"]["tasks"]:
        task["title"] = "批次 A 冲突"
    status_conflict, conflict = request(d, "POST", "/v1/data/import", d["tokens"]["ui"], {
        "bundles": [bundle_a, modified], "idempotency_key": "batch-conflict",
    })
    d_tasks = tasks(d)
    result = {
        "batch_status": status_batch,
        "batch": batch,
        "tasks_after_batch": sorted(row["task"]["title"] for row in c_tasks),
        "replay_status": status_replay,
        "replay": replay,
        "conflict_status": status_conflict,
        "conflict": conflict,
        "target_d_tasks_after_conflict": len(d_tasks),
    }
    (EVID / "dat04-batch-import.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "batch_status": status_batch,
        "added": batch.get("added") if isinstance(batch, dict) else None,
        "tasks_after_batch": result["tasks_after_batch"],
        "replay_status": status_replay,
        "replay_replayed": replay.get("replayed") if isinstance(replay, dict) else None,
        "conflict_status": status_conflict,
        "target_d_tasks_after_conflict": len(d_tasks),
    }, ensure_ascii=False, indent=2))
finally:
    for proc, log in zip(procs, logs):
        log.close()
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()
