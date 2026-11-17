import json
import os
import pathlib
import shutil
import sqlite3
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "a3-recovery-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
env = os.environ.copy()
env["OLLAMA_API_KEY"] = "ollama"
env["ZODO_LOCAL_MODEL"] = "aravhawk/qwen3.5-opus-4.6:latest"
env["ZODO_LOCAL_MODEL_URL"] = "http://127.0.0.1:11434/v1"
log1 = (EVID / "a3-recovery-server-1.log").open("w", encoding="utf-8")
log2 = (EVID / "a3-recovery-server-2.log").open("w", encoding="utf-8")


def start(log):
    proc = subprocess.Popen(
        [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
        stdout=log,
        stderr=subprocess.STDOUT,
        env=env,
    )
    info = None
    for _ in range(200):
        time.sleep(0.1)
        path = DATA / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                if info.get("pid") == proc.pid:
                    return proc, info
            except Exception:
                pass
    proc.kill()
    raise RuntimeError("runtime did not start")


def call(info, method, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {info['tokens']['ui']}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def query(info, name, payload):
    return call(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1",
        "request_id": f"a3-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": name,
        "payload": payload,
    })


proc, info = start(log1)
try:
    status, created = call(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1",
        "request_id": f"a3-{uuid.uuid4()}",
        "idempotency_key": f"a3-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "operation": "goal.create",
        "operation_version": 1,
        "payload": {"goal": {"title": "恢复测试目标", "success_criteria": "生成三个可执行任务"}},
    })
    status, goals = query(info, "goals.list", {})
    goal_id = next(g["id"] for g in goals["data"]["goals"] if g["title"] == "恢复测试目标")
    status, created = call(info, "POST", "/v1/runs", {
        "intent": "goal.decompose",
        "input": {"goal_id": goal_id, "horizon_end": "2026-11-15T23:59:59Z"},
        "idempotency_key": "recovery-run-1",
    })
    run_id = created["run"]["id"]
    running = None
    deadline = time.time() + 30
    while time.time() < deadline:
        qs, body = call(info, "GET", f"/v1/runs/{run_id}")
        if qs == 200 and body["run"]["state"] == "running":
            running = body["run"]
            break
        time.sleep(0.2)
    if not running:
        raise RuntimeError("run never entered running state")
    proc.kill()
    proc.wait(timeout=10)
    log1.close()
    time.sleep(1)
    proc2, info2 = start(log2)
    try:
        run = None
        states = []
        deadline = time.time() + 180
        while time.time() < deadline:
            qs, body = call(info2, "GET", f"/v1/runs/{run_id}")
            if qs == 200:
                run = body["run"]
                states.append(run["state"])
                if run["state"] in ("succeeded", "failed", "cancelled"):
                    break
            time.sleep(1)
        qs, proposals = query(info2, "proposals.list", {})
        matching = [
            p for p in proposals["data"]["proposals"]
            if p.get("actor_id") == "ai_planner" and p.get("intent", "").find("恢复测试目标") >= 0
        ]
        db = sqlite3.connect(DATA / "zodo.sqlite3")
        db.row_factory = sqlite3.Row
        steps = [dict(row) for row in db.execute("SELECT seq, kind FROM run_steps WHERE run_id=? ORDER BY seq", (run_id,))]
        db.close()
        result = {
            "run_id": run_id,
            "goal_id": goal_id,
            "killed_instance": info["instance_id"],
            "restarted_instance": info2["instance_id"],
            "observed_states": states,
            "final": run,
            "ai_proposals": matching,
            "proposal_count": len(matching),
            "steps": steps,
        }
        (EVID / "a3-run-recovery.json").write_text(
            json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
        )
        print(json.dumps({
            "run_id": run_id,
            "states": states,
            "final_state": run["state"] if run else None,
            "output": run.get("output") if run else None,
            "error": run.get("error") if run else None,
            "ai_proposal_count": len(matching),
            "steps": steps,
            "instance_changed": info["instance_id"] != info2["instance_id"],
        }, ensure_ascii=False, indent=2))
    finally:
        proc2.terminate()
        try:
            proc2.wait(timeout=5)
        except Exception:
            proc2.kill()
        log2.close()
finally:
    if proc.poll() is None:
        proc.kill()
