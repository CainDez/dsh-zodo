import json
import os
import pathlib
import shutil
import subprocess
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "pln16-calendar"
EVID = ROOT / "artifacts" / "evidence" / "a4"
EVID.mkdir(parents=True, exist_ok=True)
captured = []
PORT = 39162


class Stub(BaseHTTPRequestHandler):
    def log_message(self, *args):
        return

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(length).decode("utf-8"))
        external_id = self.headers.get("Idempotency-Key")
        existing = any(row["external_id"] == external_id for row in captured)
        captured.append({"external_id": external_id, "body": body, "duplicate": existing})
        status = 409 if existing else 200
        payload = json.dumps({"ok": not existing, "external_id": external_id}).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


server = ThreadingHTTPServer(("127.0.0.1", PORT), Stub)
threading.Thread(target=server.serve_forever, daemon=True).start()
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
log = (EVID / "pln16-server.log").open("w", encoding="utf-8")
env = {**os.environ, "ZODO_CALENDAR_WRITE_URL": f"http://127.0.0.1:{PORT}/write"}
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT, env=env)


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


def command(info, operation, payload, key):
    return call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": key, "idempotency_key": key,
        "workspace_id": "ws_local", "operation": operation, "operation_version": 1,
        "payload": payload,
    })


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

    status_first, first = command(info, "commitment.create", {"commitment": {
        "title": "外部日历会议一", "start": "2026-10-26T01:00:00Z", "end": "2026-10-26T02:00:00Z",
        "external_id": "ext-meeting-1",
    }}, "pln16-first")
    deadline = time.time() + 10
    while time.time() < deadline and not any(row["external_id"] == "ext-meeting-1" for row in captured):
        time.sleep(0.1)
    status_duplicate, duplicate = command(info, "commitment.create", {"commitment": {
        "title": "重复外部会议", "start": "2026-10-27T01:00:00Z", "end": "2026-10-27T02:00:00Z",
        "external_id": "ext-meeting-1",
    }}, "pln16-duplicate")
    status_second, second = command(info, "commitment.create", {"commitment": {
        "title": "外部日历会议二", "start": "2026-10-28T01:00:00Z", "end": "2026-10-28T02:00:00Z",
        "external_id": "ext-meeting-2",
    }}, "pln16-second")
    deadline = time.time() + 10
    while time.time() < deadline and len(captured) < 2:
        time.sleep(0.1)
    result = {
        "first_status": status_first,
        "first": first,
        "duplicate_status": status_duplicate,
        "duplicate": duplicate,
        "second_status": status_second,
        "captured": captured,
        "external_writes": len(captured),
        "idempotency_keys": [row["external_id"] for row in captured],
    }
    (EVID / "pln16-calendar-write.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "first_status": status_first,
        "duplicate_status": status_duplicate,
        "duplicate_code": (duplicate.get("error") or {}).get("code"),
        "second_status": status_second,
        "external_writes": len(captured),
        "idempotency_keys": result["idempotency_keys"],
        "payload_titles": [row["body"].get("title") for row in captured],
    }, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
    server.shutdown()
