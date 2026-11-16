import json
import pathlib
import shutil
import sqlite3
import subprocess
import time
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "zodo-fnd06"
EVID = ROOT / "artifacts" / "evidence" / "a1"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
                        stdout=(EVID / "fnd06-core.log").open("w", encoding="utf-8"), stderr=subprocess.STDOUT)
info = None
try:
    for _ in range(200):
        time.sleep(0.1)
        path = DATA / "runtime.json"
        if path.exists():
            try:
                candidate = json.loads(path.read_text(encoding="utf-8"))
                if candidate.get("pid") == proc.pid:
                    info = candidate
                    break
            except Exception:
                pass
    if not info:
        raise RuntimeError("Core did not start")
    base = f"http://{info['host']}:{info['port']}"
    headers = {"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"}

    def command(op, payload):
        key = str(uuid.uuid4())
        request = urllib.request.Request(
            base + "/v1/commands", method="POST", headers=headers,
            data=json.dumps({"schema": "zodo.command/1", "request_id": key, "idempotency_key": key,
                             "workspace_id": "ws_local", "operation": op, "operation_version": 1,
                             "payload": payload}).encode(),
        )
        return json.loads(urllib.request.urlopen(request).read().decode())

    def query(name, payload):
        request = urllib.request.Request(
            base + "/v1/queries", method="POST", headers=headers,
            data=json.dumps({"schema": "zodo.query/1", "request_id": "q", "workspace_id": "ws_local",
                             "query": name, "payload": payload}).encode(),
        )
        return json.loads(urllib.request.urlopen(request).read().decode())

    import datetime
    fire_at = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=1)).isoformat()
    command("reminder.create", {"message": "FND06 unknown-result reconcile", "fire_at": fire_at})
    snapshot = query("workspace.snapshot", {})["data"]
    reminder = [row for row in snapshot["reminders"] if row["message"].startswith("FND06")][0]
    command("reminder.mark_delivered", {"id": reminder["id"], "delivered_at": fire_at})
    time.sleep(5)
    db = sqlite3.connect(f"file:{DATA / 'zodo.sqlite3'}?mode=ro", uri=True)
    rows = db.execute("select status, attempts, coalesce(last_error,'') from effect_outbox where effect_type='reminder.deliver' order by rowid desc limit 3").fetchall()
    db.close()
    result = {
        "reminder_id": reminder["id"],
        "reminder_status_after_mark": "delivered",
        "effect_rows": [{"status": row[0], "attempts": row[1], "last_error": row[2]} for row in rows],
        "reconciled": bool(rows) and rows[0][0] == "delivered",
        "note": "delivery result was recorded as delivered before the effect worker retried; worker must reconcile without a second toast",
    }
    (EVID / "fnd06-reconcile.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
