import json
import pathlib
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
EVID = ROOT / "artifacts" / "evidence" / "a3"
info = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
base = f"http://{info['host']}:{info['port']}"
token = info["tokens"]["ui"]


def call(method, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def query(name, payload):
    return call("POST", "/v1/queries", {
        "schema": "zodo.query/1",
        "request_id": f"a3-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": name,
        "payload": payload,
    })


status, goals = query("goals.list", {})
if status != 200:
    raise RuntimeError(f"goals query failed: {goals}")
goal_id = goals["data"]["goals"][0]["id"]
status, created = call("POST", "/v1/runs", {
    "intent": "goal.decompose",
    "input": {"goal_id": goal_id, "horizon_end": "2026-11-15T23:59:59Z"},
    "idempotency_key": f"cancel-{uuid.uuid4()}",
})
if status not in (200, 202):
    raise RuntimeError(f"run create failed: {created}")
run_id = created["run"]["id"]
status, cancelled = call("POST", f"/v1/runs/{run_id}/cancel", {})
states = []
deadline = time.time() + 30
run = None
while time.time() < deadline:
    qs, body = call("GET", f"/v1/runs/{run_id}")
    if qs == 200:
        run = body["run"]
        states.append(run["state"])
        if run["state"] in ("cancelled", "failed", "succeeded"):
            break
    time.sleep(0.5)
result = {
    "run_id": run_id,
    "cancel_http": status,
    "cancel_body": cancelled,
    "observed_states": states,
    "final": run,
}
(EVID / "a3-run-cancel.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({
    "run_id": run_id,
    "cancel_http": status,
    "states": states,
    "final_state": run["state"] if run else None,
    "error": run.get("error") if run else None,
}, ensure_ascii=False, indent=2))
