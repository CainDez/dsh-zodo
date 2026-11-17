import json
import pathlib
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
info = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
base = f"http://{info['host']}:{info['port']}"
ui = info["tokens"]["ui"]


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {ui}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


dedupe = f"a3-notify-{uuid.uuid4()}"
envelope = {
    "schema": "zodo.command/1",
    "request_id": f"a3-notify-{uuid.uuid4()}",
    "idempotency_key": f"a3-notify-{uuid.uuid4()}",
    "workspace_id": "ws_local",
    "operation": "reminder.create",
    "operation_version": 1,
    "payload": {
        "task_id": None,
        "message": "A3 真实提醒：本地 Core 已完成通知投递",
        "fire_at": datetime.now(timezone.utc).isoformat(),
        "dedupe_key": dedupe,
    },
}
status, created = request("POST", "/v1/commands", envelope)
if status >= 300:
    raise RuntimeError(f"reminder create failed {status}: {created}")
payload = created.get("data", {})
reminder_id = None
if "change" in payload:
    reminder_id = payload["change"].get("id")
elif "changes" in payload:
    for change in payload["changes"]:
        if change.get("kind") == "reminder":
            reminder_id = change.get("id")
if not reminder_id:
    # Query the reminders list and find by dedupe.
    q = {
        "schema": "zodo.query/1",
        "request_id": f"a3-query-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": "reminders.list",
        "payload": {},
    }
    qs, qb = request("POST", "/v1/queries", q)
    for reminder in qb.get("data", {}).get("reminders", []):
        if reminder.get("dedupe_key") == dedupe:
            reminder_id = reminder["id"]
            break
if not reminder_id:
    raise RuntimeError(f"could not find reminder id in {json.dumps(created)[:1000]}")
state = None
deadline = time.time() + 20
while time.time() < deadline:
    q = {
        "schema": "zodo.query/1",
        "request_id": f"a3-query-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": "reminders.list",
        "payload": {},
    }
    qs, qb = request("POST", "/v1/queries", q)
    for reminder in qb.get("data", {}).get("reminders", []):
        if reminder.get("id") == reminder_id:
            state = reminder
            break
    if state and state.get("status") in ("delivered", "failed"):
        break
    time.sleep(0.5)
notifications = []
log_path = ROOT / ".runtime" / "zodo-a2" / "notifications.jsonl"
if log_path.exists():
    for line in log_path.read_text(encoding="utf-8").splitlines():
        try:
            row = json.loads(line)
        except Exception:
            continue
        if row.get("reminder_id") == reminder_id:
            notifications.append(row)
result = {
    "reminder_id": reminder_id,
    "create_status": status,
    "state": state,
    "notifications": notifications,
    "log_exists": log_path.exists(),
}
(EVID / "a3-notification.json").write_text(
    json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
)
print(json.dumps(result, ensure_ascii=False, indent=2))
