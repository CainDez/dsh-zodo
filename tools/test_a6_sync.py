import json
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
EVID = ROOT / "artifacts" / "evidence" / "a6"
EVID.mkdir(parents=True, exist_ok=True)


def start(name):
    data = ROOT / ".runtime" / f"a6-{name}"
    if data.exists():
        shutil.rmtree(data)
    data.mkdir(parents=True)
    log = (EVID / f"a6-{name}.log").open("w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(Z), "--data-dir", str(data), "serve", "--port", "0"],
        stdout=log, stderr=subprocess.STDOUT,
    )
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
        raise RuntimeError(f"{name} failed to start")
    return proc, info, log


def request(info, method, path, token, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path,
        data=data, method=method,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def command(info, operation, payload):
    return request(info, "POST", "/v1/commands", info["tokens"]["ui"], {
        "schema": "zodo.command/1",
        "request_id": f"a6-{time.time_ns()}",
        "idempotency_key": f"a6-{time.time_ns()}",
        "workspace_id": "ws_local",
        "operation": operation,
        "operation_version": 1,
        "payload": payload,
    })


def task_list(info):
    qs, body = request(info, "POST", "/v1/queries", info["tokens"]["ui"], {
        "schema": "zodo.query/1", "request_id": "a6-list", "workspace_id": "ws_local",
        "query": "tasks.list", "payload": {"filter": {"limit": 100}},
    })
    return body["data"]["tasks"]


def task_payload(task, title):
    return {
        "title": title,
        "description": task.get("description"),
        "project_id": task.get("project_id"),
        "parent_id": task.get("parent_id"),
        "estimate_minutes": task.get("estimate_minutes"),
        "priority": task.get("priority", 2),
        "target_due": task.get("target_due"),
        "hard_deadline": task.get("hard_deadline"),
        "not_before": task.get("not_before"),
        "preferred_window": task.get("preferred_window"),
        "tags": task.get("tags", []),
    }


a, ai, alog = start("a")
b, bi, blog = start("b")
try:
    command(ai, "task.create", {"task": {"title": "设备 A 的任务", "estimate_minutes": 10}})
    command(bi, "task.create", {"task": {"title": "设备 B 的任务", "estimate_minutes": 10}})
    status, bundle_a = request(ai, "GET", "/v1/sync/export", ai["tokens"]["ui"])
    status_b, import_b = request(bi, "POST", "/v1/sync/import", bi["tokens"]["ui"], {"bundle": bundle_a})
    b_tasks = task_list(bi)
    status_bundle_b, bundle_b = request(bi, "GET", "/v1/sync/export", bi["tokens"]["ui"])
    status_a, import_a = request(ai, "POST", "/v1/sync/import", ai["tokens"]["ui"], {"bundle": bundle_b})
    a_tasks = task_list(ai)
    # Diverge the shared task on both devices, then import A into B: must conflict without writes.
    shared = next(row["task"] for row in a_tasks if row["task"]["title"] == "设备 A 的任务")
    command(ai, "task.update", {"id": shared["id"], "task": task_payload(shared, "A 修改后的标题"), "expected_version": shared["version"]})
    b_shared = next(task for task in task_list(bi) if task["task"]["id"] == shared["id"])["task"]
    command(bi, "task.update", {"id": b_shared["id"], "task": task_payload(b_shared, "B 修改后的标题"), "expected_version": b_shared["version"]})
    status, bundle_a2 = request(ai, "GET", "/v1/sync/export", ai["tokens"]["ui"])
    status_conflict, conflict = request(bi, "POST", "/v1/sync/import", bi["tokens"]["ui"], {"bundle": bundle_a2})
    b_after = task_list(bi)
    # Explicit conflict resolution: choose the incoming A fact, then retry atomically.
    conflict_id = conflict.get("error", {}).get("details", {}).get("conflicts", [{}])[0].get("id")
    status_retry, retry = request(bi, "POST", "/v1/sync/import", bi["tokens"]["ui"], {
        "bundle": bundle_a2,
        "resolutions": {conflict_id: "incoming"},
    })
    b_retry = task_list(bi)
    # Device revocation must reject the whole bundle; unrevoking restores the channel.
    device_b = bundle_b.get("device_id")
    revoke_status, revoke_body = request(ai, "POST", "/v1/sync/revoke", ai["tokens"]["ui"], {"device_id": device_b, "revoked": True})
    revoked_import_status, revoked_import = request(ai, "POST", "/v1/sync/import", ai["tokens"]["ui"], {"bundle": bundle_b})
    unrevoke_status, unrevoke_body = request(ai, "POST", "/v1/sync/revoke", ai["tokens"]["ui"], {"device_id": device_b, "revoked": False})
    unrevoked_import_status, unrevoked_import = request(ai, "POST", "/v1/sync/import", ai["tokens"]["ui"], {"bundle": bundle_b})
    result = {
        "export_a_status": status,
        "import_b_status": status_b,
        "import_b": import_b,
        "b_tasks_after_first_sync": len(b_tasks),
        "export_b_status": status_bundle_b,
        "import_a_status": status_a,
        "import_a": import_a,
        "a_tasks_after_roundtrip": len(a_tasks),
        "conflict_status": status_conflict,
        "conflict": conflict,
        "b_tasks_after_conflict": len(b_after),
        "b_shared_title_unchanged": next(row["task"] for row in b_after if row["task"]["id"] == shared["id"])["title"],
        "retry_status": status_retry,
        "retry_body": retry,
        "b_retry_shared_title": next(row["task"] for row in b_retry if row["task"]["id"] == shared["id"])["title"],
        "device_a": bundle_a.get("device_id"),
        "device_b": bundle_b.get("device_id"),
        "revoke_status": revoke_status,
        "revoke_body": revoke_body,
        "revoked_import_status": revoked_import_status,
        "revoked_import": revoked_import,
        "unrevoke_status": unrevoke_status,
        "unrevoke_body": unrevoke_body,
        "unrevoked_import_status": unrevoked_import_status,
        "unrevoked_import": unrevoked_import,
    }
    (EVID / "a6-swap.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "a_tasks_after_roundtrip": len(a_tasks),
        "b_tasks_after_first_sync": len(b_tasks),
        "import_b_added": import_b.get("added"),
        "import_a_added": import_a.get("added"),
        "conflict_status": status_conflict,
        "conflict_error": conflict.get("error", {}).get("code"),
        "conflict_count": len(conflict.get("error", {}).get("details", {}).get("conflicts", [])) if isinstance(conflict.get("error", {}).get("details"), dict) else None,
        "b_shared_title_unchanged": result["b_shared_title_unchanged"],
        "devices_differ": result["device_a"] != result["device_b"],
    }, ensure_ascii=False, indent=2))
finally:
    for proc, log in ((a, alog), (b, blog)):
        log.close()
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()
