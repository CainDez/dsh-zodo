import json
import os
import pathlib
import sqlite3
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "a3-model-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    import shutil
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)

env = os.environ.copy()
env["OLLAMA_API_KEY"] = "ollama"
env["ZODO_LOCAL_MODEL"] = "aravhawk/qwen3.5-opus-4.6:latest"
env["ZODO_LOCAL_MODEL_URL"] = "http://127.0.0.1:11434/v1"
server_log = (EVID / "a3-model-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen(
    [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
    stdout=server_log,
    stderr=subprocess.STDOUT,
    env=env,
)
try:
    info = None
    for _ in range(150):
        time.sleep(0.1)
        path = DATA / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                break
            except Exception:
                pass
    if not info:
        raise RuntimeError("runtime did not start")
    base = f"http://{info['host']}:{info['port']}"
    token = info["tokens"]["ui"]

    def request(method, suffix, body):
        data = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            base + suffix,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=180) as response:
                return response.status, json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read().decode("utf-8"))

    def command(operation, payload, key=None):
        envelope = {
            "schema": "zodo.command/1",
            "request_id": f"a3-{uuid.uuid4()}",
            "idempotency_key": key or f"a3-{uuid.uuid4()}",
            "workspace_id": "ws_local",
            "operation": operation,
            "operation_version": 1,
            "payload": payload,
        }
        status, body = request("POST", "/v1/commands", envelope)
        if status >= 300:
            raise RuntimeError(f"command {operation} failed {status}: {body}")
        return body

    goal_body = command(
        "goal.create",
        {
            "goal": {
                "title": "完成作品集第三版并上线",
                "success_criteria": "完成三个案例、首页和在线发布检查",
                "target_due": "2026-11-15",
            }
        },
    )
    goals_envelope = {
        "schema": "zodo.query/1",
        "request_id": f"a3-query-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": "goals.list",
        "payload": {},
    }
    goals_status, goals_body = request("POST", "/v1/queries", goals_envelope)
    if goals_status != 200:
        raise RuntimeError(f"goals query failed {goals_status}: {goals_body}")
    goal_id = next(
        goal["id"]
        for goal in goals_body["data"]["goals"]
        if goal["title"] == "完成作品集第三版并上线"
    )
    model_status, model_body = request("GET", "/v1/model", None)
    create_status, create_body = request(
        "POST",
        "/v1/runs",
        {
            "intent": "goal.decompose",
            "input": {"goal_id": goal_id, "horizon_end": "2026-11-15T23:59:59Z"},
            "idempotency_key": "a3-goal-decompose-1",
        },
    )
    if create_status not in (200, 202):
        raise RuntimeError(f"run create failed {create_status}: {create_body}")
    run_id = create_body["run"]["id"]
    run = None
    started = time.time()
    while time.time() - started < 180:
        status, body = request("GET", f"/v1/runs/{run_id}", None)
        if status == 200:
            run = body["run"]
            if run["state"] in ("succeeded", "failed", "cancelled"):
                break
        time.sleep(1)
    proposals = command("proposal.validate", {"proposal_id": "nonexistent"}) if False else None
    query_envelope = {
        "schema": "zodo.query/1",
        "request_id": f"a3-query-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": "proposals.list",
        "payload": {},
    }
    status, proposals_body = request("POST", "/v1/queries", query_envelope)
    proposal = None
    if status == 200 and proposals_body.get("data", {}).get("proposals"):
        proposal = proposals_body["data"]["proposals"][-1]
        proposal_id = proposal["id"]
        get_envelope = {
            "schema": "zodo.query/1",
            "request_id": f"a3-query-{uuid.uuid4()}",
            "workspace_id": "ws_local",
            "query": "proposal.get",
            "payload": {"id": proposal_id},
        }
        status, proposal_body = request("POST", "/v1/queries", get_envelope)
        if status == 200:
            proposal = proposal_body["data"]["proposal"]
    db = sqlite3.connect(DATA / "zodo.sqlite3")
    db.row_factory = sqlite3.Row
    runs = [dict(row) for row in db.execute("SELECT * FROM runs")]
    steps = [dict(row) for row in db.execute("SELECT * FROM run_steps ORDER BY seq")]
    db.close()
    output = {
        "instance": info["instance_id"],
        "goal_id": goal_id,
        "model_status": model_status,
        "model_config": model_body,
        "run_create_status": create_status,
        "run_id": run_id,
        "run": run,
        "proposal": proposal,
        "runs": runs,
        "steps": steps,
        "elapsed_seconds": round(time.time() - started, 2),
    }
    (EVID / "a3-model-run.json").write_text(
        json.dumps(output, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps({
        "run_id": run_id,
        "state": run["state"] if run else None,
        "output": run.get("output") if run else None,
        "error": run.get("error") if run else None,
        "proposal_id": proposal.get("id") if proposal else None,
        "proposal_actor": proposal.get("actor_kind") if proposal else None,
        "proposal_intent": proposal.get("intent") if proposal else None,
        "step_count": len(steps),
    }, ensure_ascii=False, indent=2))
finally:
    server_log.close()
    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
