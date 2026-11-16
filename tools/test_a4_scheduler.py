import json
import pathlib
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
INFO = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
BASE = f"http://{INFO['host']}:{INFO['port']}"
TOKEN = INFO["tokens"]["ui"]


def call(method, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        BASE + path,
        data=data,
        method=method,
        headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def command(operation, payload):
    return call("POST", "/v1/commands", {
        "schema": "zodo.command/1",
        "request_id": f"sched-{uuid.uuid4()}",
        "idempotency_key": f"sched-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "operation": operation,
        "operation_version": 1,
        "payload": payload,
    })


def snapshot():
    return call("POST", "/v1/queries", {
        "schema": "zodo.query/1",
        "request_id": f"sched-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": "workspace.snapshot",
        "payload": {},
    })


title = f"调度器自动任务 {int(time.time())}"
status, created = command("automation.create", {
    "automation": {
        "name": f"调度器测试 {int(time.time())}",
        "trigger": "schedule",
        "conditions": {"interval_seconds": 60},
        "actions": [{"kind": "task.create", "task": {"title": title, "estimate_minutes": 5}}],
        "enabled": True,
        "budget": 2,
    },
})
if status >= 300:
    raise RuntimeError(created)
rule_id = created["data"]["automation"]["id"]
deadline = time.time() + 20
rule = None
task_found = False
while time.time() < deadline:
    time.sleep(1)
    qs, body = snapshot()
    if qs != 200:
        continue
    data = body["data"]
    rule = next((r for r in data.get("automation_rules", []) if r["id"] == rule_id), None)
    task_found = any(t["title"] == title for t in data.get("tasks", []))
    if rule and rule["runs"] >= 1 and task_found:
        break
result = {
    "rule_id": rule_id,
    "title": title,
    "rule": rule,
    "task_found": task_found,
    "create_status": status,
}
out = ROOT / "artifacts" / "evidence" / "a4"
(out / "a4-automation-schedule.json").write_text(
    json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
)
print(json.dumps({
    "rule_id": rule_id,
    "runs": rule["runs"] if rule else None,
    "last_run_at": rule["last_run_at"] if rule else None,
    "task_found": task_found,
    "enabled": rule["enabled"] if rule else None,
}, ensure_ascii=False, indent=2))
