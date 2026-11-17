"""Admitted synthetic custom-model case; execute only through run_isolated_integration.py.

The runner relocates ROOT/PORT, removes inherited secrets and enforces owned services.
Do not execute or import this source directly.
"""
import json
import pathlib
import shutil
import subprocess
import threading
import time
import urllib.error
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "ai11-custom-stub"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
captured = []
PORT = 39091


class Stub(BaseHTTPRequestHandler):
    def log_message(self, *args):
        return

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length).decode("utf-8")
        captured.append({"path": self.path, "authorization_present": bool(self.headers.get("Authorization")), "body": json.loads(body)})
        content = json.dumps({
            "tasks": [
                {"title": "云路由任务一", "estimate_minutes": 30, "priority": 2, "target_due": None, "depends_on": []},
                {"title": "云路由任务二", "estimate_minutes": 45, "priority": 1, "target_due": None, "depends_on": [0]},
                {"title": "云路由任务三", "estimate_minutes": 20, "priority": 2, "target_due": None, "depends_on": [1]},
            ],
            "assumptions": ["stub cloud route"],
            "questions": [],
        }, ensure_ascii=False)
        payload = json.dumps({"choices": [{"message": {"role": "assistant", "content": content}}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


server = ThreadingHTTPServer(("127.0.0.1", PORT), Stub)
threading.Thread(target=server.serve_forever, daemon=True).start()
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
env = {"OLLAMA_API_KEY": "ollama", "DEEPSEEK_API_KEY": "stub-cloud-key"}
log = (EVID / "ai11-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen(
    [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
    stdout=log, stderr=subprocess.STDOUT, env={**__import__("os").environ, **env},
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
    goal_status, goal_body = call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": "ai11", "idempotency_key": "ai11-goal",
        "workspace_id": "ws_local", "operation": "goal.create", "operation_version": 1,
        "payload": {"goal": {"title": "自定义模型目标", "success_criteria": "产出三个任务"}},
    })
    goal_id = goal_body["data"]["changes"][0]["id"]
    call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": "ai11-note", "idempotency_key": "ai11-note",
        "workspace_id": "ws_local", "operation": "note.create", "operation_version": 1,
        "payload": {"note": {"title": "私密笔记", "body": "device-only secret body", "data_class": "device_only"}},
    })
    call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": "ai11-policy", "idempotency_key": "ai11-policy",
        "workspace_id": "ws_local", "operation": "workspace.update", "operation_version": 1,
        "payload": {"settings": None, "policy": {
            "model_access": {"goals": True, "projects": False, "captures": False},
            "diagnostic_capture": False,
        }},
    })
    model_status, model_body = call(info, "POST", "/v1/model", {
        "version": 2, "source": {
            "kind": "custom", "base_url": f"http://127.0.0.1:{PORT}/v1",
            "model": "stub-custom-model", "api_key_env": "DEEPSEEK_API_KEY",
        },
    })
    run_status, run_body = call(info, "POST", "/v1/runs", {
        "intent": "goal.decompose", "input": {"goal_id": goal_id}, "idempotency_key": "ai11-run",
    })
    run_id = run_body["run"]["id"]
    run = None
    end = time.time() + 60
    while time.time() < end:
        time.sleep(0.5)
        get_status, body = call(info, "GET", f"/v1/runs/{run_id}")
        if get_status == 200:
            run = body["run"]
            if run["state"] in ("succeeded", "failed", "cancelled", "interrupted"):
                break
    proposal_id = (run or {}).get("output", {}).get("proposal_id")
    approved_status = None
    committed_status = None
    dependencies = []
    if run and run.get("state") == "succeeded" and proposal_id:
        approved_status, approved = call(info, "POST", "/v1/commands", {
            "schema": "zodo.command/1", "request_id": "ai11-approve", "idempotency_key": "ai11-approve",
            "workspace_id": "ws_local", "operation": "proposal.approve", "operation_version": 1,
            "payload": {"proposal_id": proposal_id},
        })
        grant = (approved.get("data") or {}).get("approval", {}).get("id")
        committed_status, committed = call(info, "POST", "/v1/commands", {
            "schema": "zodo.command/1", "request_id": "ai11-commit", "idempotency_key": "ai11-commit",
            "workspace_id": "ws_local", "operation": "changes.commit", "operation_version": 1,
            "payload": {"proposal_id": proposal_id, "approval_grant": grant},
        })
        snapshot_status, snapshot = call(info, "POST", "/v1/queries", {
            "schema": "zodo.query/1", "request_id": "ai11-snap", "workspace_id": "ws_local",
            "query": "workspace.snapshot", "payload": {},
        })
        tasks = (snapshot.get("data") or {}).get("tasks") or []
        by_id = {(row.get("task") or row).get("id"): (row.get("task") or row) for row in tasks}
        for dependency in (snapshot.get("data") or {}).get("dependencies", []):
            dependencies.append({
                "predecessor": by_id.get(dependency.get("predecessor_id"), {}).get("title"),
                "successor": by_id.get(dependency.get("successor_id"), {}).get("title"),
            })
    request_text = json.dumps(captured, ensure_ascii=False)
    result = {
        "goal_status": goal_status,
        "model_status": model_status,
        "model_body": model_body.get("config", {}).get("source"),
        "run_status": run_status,
        "run": run,
        "custom_requests": len(captured),
        "model_source": run.get("output", {}).get("source") if run else None,
        "model_provider": run.get("output", {}).get("provider") if run else None,
        "private_note_in_model_request": "device-only secret body" in request_text,
        "stub_received_goal": "自定义模型目标" in request_text,
        "task_count": run.get("output", {}).get("task_count") if run else None,
        "approved_status": approved_status,
        "committed_status": committed_status,
        "dependencies": dependencies,
        "evidence_request": captured[:1],
    }
    (EVID / "ai11-custom-stub.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "run_state": run.get("state") if run else None,
        "route": result["model_source"],
        "provider": result["model_provider"],
        "custom_requests": result["custom_requests"],
        "private_note_in_model_request": result["private_note_in_model_request"],
        "stub_received_goal": result["stub_received_goal"],
        "task_count": result["task_count"],
        "dependencies": result["dependencies"],
    }, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
    server.shutdown()
