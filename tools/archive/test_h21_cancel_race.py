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
DATA = ROOT / ".runtime" / "h21-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
env = os.environ.copy()
env["OLLAMA_API_KEY"] = "ollama"
log = (EVID / "h21-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT, env=env)


def request(info, method, path, body=None, token=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
        headers={"Authorization": f"Bearer {token or info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def command(info, operation, payload):
    return request(info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": f"h21-{uuid.uuid4()}", "idempotency_key": f"h21-{uuid.uuid4()}",
        "workspace_id": "ws_local", "operation": operation, "operation_version": 1, "payload": payload,
    })


def query(info, name, payload=None):
    return request(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": f"h21-{uuid.uuid4()}", "workspace_id": "ws_local",
        "query": name, "payload": payload or {},
    })


def create_run(info, goal_id):
    return request(info, "POST", "/v1/runs", {"intent": "goal.decompose", "input": {"goal_id": goal_id}, "idempotency_key": f"h21-{uuid.uuid4()}"})


def get_run(info, run_id):
    return request(info, "GET", f"/v1/runs/{run_id}")


def wait_run(info, run_id, deadline=90):
    end = time.time() + deadline
    while time.time() < end:
        status, body = get_run(info, run_id)
        if status == 200 and body.get("run", {}).get("state") in ("succeeded", "failed", "cancelled", "interrupted"):
            return body["run"]
        time.sleep(0.5)
    return None


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
    goal_status, goal_body = command(info, "goal.create", {
        "goal": {"title": "取消竞态测试目标", "success_criteria": "验证取消不会写事实"},
    })
    if goal_status >= 300:
        raise RuntimeError(goal_body)
    goal_id = goal_body["data"].get("goal", {}).get("id") or goal_body["data"]["changes"][0]["id"]

    _, proposals_before = query(info, "proposals.list")
    queued_status, queued = create_run(info, goal_id)
    run_id = queued["run"]["id"]
    cancel_status, cancelled_body = request(info, "POST", f"/v1/runs/{run_id}/cancel", {})
    cancelled = wait_run(info, run_id)
    _, proposals_after = query(info, "proposals.list")

    # Second run is allowed to reach a proposal; cancel then must not fabricate writes.
    second_status, second = create_run(info, goal_id)
    second_id = second["run"]["id"]
    proposal = None
    end = time.time() + 90
    while time.time() < end:
        _, proposals_mid = query(info, "proposals.list")
        if proposals_mid["data"].get("proposals"):
            proposal = proposals_mid["data"]["proposals"][-1]
            break
        run = get_run(info, second_id)[1].get("run", {})
        if run.get("state") in ("failed", "cancelled"):
            break
        time.sleep(0.5)
    cancel2_status, cancel2 = request(info, "POST", f"/v1/runs/{second_id}/cancel", {})
    second_run = wait_run(info, second_id)
    _, proposals_final = query(info, "proposals.list")
    committed = None
    if proposal:
        approve = command(info, "proposal.approve", {"proposal_id": proposal["id"]})
        if approve[0] < 300:
            grant = approve[1]["data"]["approval"]["id"]
            committed = command(info, "changes.commit", {"proposal_id": proposal["id"], "approval_grant": grant})
    result = {
        "goal_id": goal_id,
        "queued_create": queued_status,
        "first_cancel_status": cancel_status,
        "first_run_state": cancelled["state"] if cancelled else None,
        "first_cancel_requested": cancelled["cancel_requested"] if cancelled else None,
        "proposals_before": len(proposals_before["data"].get("proposals", [])),
        "proposals_after_queued_cancel": len(proposals_after["data"].get("proposals", [])),
        "second_run_id": second_id,
        "second_run_reached_proposal": bool(proposal),
        "second_cancel_status": cancel2_status,
        "second_cancel_code": cancel2.get("error", {}).get("code") if isinstance(cancel2, dict) else None,
        "second_run_state": second_run["state"] if second_run else None,
        "proposals_final": len(proposals_final["data"].get("proposals", [])),
        "approve_status": approve[0] if proposal else None,
        "commit_status": committed[0] if committed else None,
        "commit_revision": committed[1].get("revision") if committed else None,
    }
    (EVID / "h21-cancel-race.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
