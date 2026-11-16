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
DATA = ROOT / ".runtime" / "pln14-notify-retry"
EVID = ROOT / "artifacts" / "evidence" / "a4"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
log = (EVID / "pln14-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen(
    [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
    stdout=log, stderr=subprocess.STDOUT,
)


def call(info, method, path, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode()
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
        headers={"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def notifications():
    path = DATA / "notifications.jsonl"
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


try:
    info = None
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
        raise RuntimeError("core did not start")

    script = DATA / "zodo-toast.ps1"
    for _ in range(100):
        if script.exists():
            break
        time.sleep(0.05)
    original_script = script.read_text(encoding="utf-8")
    fire_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() + 2))
    status, body = call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": "pln14", "idempotency_key": "pln14-reminder",
        "workspace_id": "ws_local", "operation": "reminder.create", "operation_version": 1,
        "payload": {"message": "PLN14 通知恢复测试", "fire_at": fire_at, "dedupe_key": "pln14-notification"},
    })
    script.write_text("exit 1\n", encoding="utf-8")
    failure = None
    for _ in range(80):
        time.sleep(0.2)
        rows = [row for row in notifications() if row.get("message") == "PLN14 通知恢复测试"]
        if rows:
            failure = rows[-1]
            if failure.get("delivered") is False:
                break
    script.write_text(original_script, encoding="utf-8")
    recovery = None
    for _ in range(220):
        time.sleep(0.2)
        rows = [row for row in notifications() if row.get("message") == "PLN14 通知恢复测试"]
        if rows:
            recovery = rows[-1]
            if recovery.get("delivered") is True:
                break
    result = {
        "reminder_status": status,
        "reminder_body": body,
        "failure_log": failure,
        "recovery_log": recovery,
        "failure_delivered": failure.get("delivered") if failure else None,
        "recovery_delivered": recovery.get("delivered") if recovery else None,
        "script_restored": "TOAST_OK" in script.read_text(encoding="utf-8"),
    }
    (EVID / "pln14-notify-retry.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "reminder_status": status,
        "failure_delivered": result["failure_delivered"],
        "failure_error": (failure or {}).get("error"),
        "recovery_delivered": result["recovery_delivered"],
        "recovery_backend": (recovery or {}).get("backend"),
    }, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
