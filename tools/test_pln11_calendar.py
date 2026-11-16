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
DATA = ROOT / ".runtime" / "pln11-test"
EVID = ROOT / "artifacts" / "evidence" / "a4"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
log = (EVID / "pln11-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)


def call(info, method, path, body=None):
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


def command(info, payload):
    return call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": f"pln11-{uuid.uuid4()}", "idempotency_key": f"pln11-{uuid.uuid4()}",
        "workspace_id": "ws_local", "operation": "calendar.import_busy", "operation_version": 1, "payload": payload,
    })


def snapshot(info):
    status, body = call(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": "pln11-q", "workspace_id": "ws_local",
        "query": "workspace.snapshot", "payload": {},
    })
    return body["data"]


def event(title, start, end, external_id=None):
    return {"external_id": external_id or f"ext-{title}", "title": title, "start": start, "end": end}


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
    first_status, first = command(info, {
        "provider": "ics", "display_name": "测试日历", "external_id": "cal-1",
        "events": [
            event("会议一", "2026-11-02T01:00:00Z", "2026-11-02T02:00:00Z"),
            event("会议二", "2026-11-03T01:00:00Z", "2026-11-03T02:00:00Z"),
        ],
        "replace_source": False,
    })
    snap1 = snapshot(info)
    source_id = snap1["calendar_sources"][0]["id"]
    second_status, second = command(info, {
        "source_id": source_id, "provider": "ics", "display_name": "测试日历", "external_id": "cal-1",
        "events": [
            event("会议二更新", "2026-11-03T01:00:00Z", "2026-11-03T02:30:00Z", "ext-会议二"),
            event("会议三", "2026-11-04T01:00:00Z", "2026-11-04T02:00:00Z"),
        ],
        "replace_source": False,
    })
    snap2 = snapshot(info)
    replace_status, replace = command(info, {
        "source_id": source_id, "provider": "ics", "display_name": "测试日历", "external_id": "cal-1",
        "events": [event("只保留", "2026-11-05T01:00:00Z", "2026-11-05T02:00:00Z")],
        "replace_source": True,
    })
    snap3 = snapshot(info)
    bad_status, bad = command(info, {
        "source_id": source_id, "provider": "ics", "display_name": "测试日历", "external_id": "cal-1",
        "events": [{"external_id": "bad", "title": "坏事件", "start": "2026-11-06T02:00:00Z", "end": "2026-11-06T01:00:00Z"}],
        "replace_source": True,
    })
    snap4 = snapshot(info)
    result = {
        "first_status": first_status,
        "first_events": len(snap1["calendar_events"]),
        "second_status": second_status,
        "second_events": len(snap2["calendar_events"]),
        "second_external_ids": sorted(item["external_id"] for item in snap2["calendar_events"]),
        "replace_status": replace_status,
        "replace_events": len(snap3["calendar_events"]),
        "bad_status": bad_status,
        "bad_code": bad.get("error", {}).get("code"),
        "events_after_bad": len(snap4["calendar_events"]),
        "source_last_sync": snap4["calendar_sources"][0].get("last_sync_at"),
    }
    (EVID / "a4-pln11-calendar.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
