"""Fresh-data API checks used by run_isolated_integration (never attaches to a Core)."""
import json
from pathlib import Path
import subprocess
import time
import traceback
import urllib.error
import urllib.request
import uuid


def run(root, binary, reject_url, guard):
    root.mkdir()
    data = root / "core"
    data.mkdir()
    checks = []
    started = time.monotonic()

    def expect(name, actual, expected):
        checks.append({"name": name, "passed": actual == expected, "actual": actual, "expected": expected})
        if actual != expected:
            raise AssertionError(f"{name}: {actual!r} != {expected!r}")

    try:
        with guard(root, binary, reject_url) as processes, (root / "core.log").open("w", encoding="utf-8") as log:
            proc = subprocess.Popen([str(binary), "--data-dir", str(data), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 15
            info = None
            while time.monotonic() < deadline:
                runtime = data / "runtime.json"
                if runtime.exists():
                    candidate = json.loads(runtime.read_text(encoding="utf-8"))
                    if candidate.get("pid") == proc.pid:
                        info = candidate
                        break
                if proc.poll() is not None:
                    raise RuntimeError("owned Core exited before startup")
                time.sleep(.1)
            if info is None:
                raise RuntimeError("owned Core startup timed out")
            base = f"http://{info['host']}:{info['port']}"
            tokens = info["tokens"]

            def call(path, method="GET", body=None, channel="ui", workspace="ws_local"):
                headers = {"Content-Type": "application/json"}
                if channel:
                    headers["Authorization"] = "Bearer " + tokens.get(channel, channel)
                req = urllib.request.Request(base+path, data=None if body is None else json.dumps(body).encode(), method=method, headers=headers)
                try:
                    with urllib.request.urlopen(req, timeout=10) as response:
                        return response.status, json.loads(response.read().decode())
                except urllib.error.HTTPError as error:
                    return error.code, json.loads(error.read().decode())

            def command(op, payload, channel="ui"):
                key = str(uuid.uuid4())
                return call("/v1/commands", "POST", {"schema":"zodo.command/1", "request_id":key, "idempotency_key":key, "workspace_id":"ws_local", "operation":op, "operation_version":1, "payload":payload}, channel)

            def query(name, payload=None, channel="ui", workspace="ws_local"):
                return call("/v1/queries", "POST", {"schema":"zodo.query/1", "request_id":str(uuid.uuid4()), "workspace_id":workspace, "query":name, "payload":payload or {}}, channel)

            def snapshot():
                status, body = query("workspace.snapshot")
                expect("snapshot available", status, 200)
                return body["data"]

            def scenario(name, function):
                try:
                    function()
                except Exception:
                    checks.append({"name":name, "passed":False, "error":traceback.format_exc()})

            def authentication():
                expect("no token denied", call("/v1/model", channel=None)[0], 403)
                expect("forged token denied", call("/v1/model", channel="forged")[0], 403)
                expect("agent admin backup denied", call("/v1/data/backup", "POST", {}, "agent")[0], 403)
                expect("agent policy update denied", command("workspace.update", {"settings":None,"policy":None}, "agent")[0], 403)
                expect("MCP private snapshot denied", query("workspace.snapshot", channel="mcp")[0], 403)
                expect("cross workspace denied", query("workspace.get", workspace="ws_other")[0], 403)
                expect("revoke agent", call("/v1/admin/revoke", "POST", {"channel":"agent", "revoked":True})[0], 200)
                expect("revoked agent denied", call("/v1/model", channel="agent")[0], 403)
                expect("UI unaffected", call("/v1/model")[0], 200)
                expect("restore agent", call("/v1/admin/revoke", "POST", {"channel":"agent", "revoked":False})[0], 200)
                expect("agent restored", call("/v1/model", channel="agent")[0], 200)

            def proposal_boundary():
                status, proposed = command("proposal.create", {"intent":"isolated security check", "operations":[{"operation":"task.create", "payload":{"task":{"title":"MCP proposed only", "estimate_minutes":15}}}]}, "mcp")
                expect("MCP may propose", status, 200)
                proposal = proposed["data"]["proposal"]["id"]
                expect("proposal writes no facts", len(snapshot()["tasks"]), 0)
                expect("MCP approval denied", command("proposal.approve", {"proposal_id":proposal}, "mcp")[0], 403)
                expect("MCP commit denied", command("changes.commit", {"proposal_id":proposal,"approval_grant":"forged"}, "mcp")[0], 403)
                expect("forged UI grant denied", command("changes.commit", {"proposal_id":proposal,"approval_grant":"forged"})[0], 409)
                expect("denial writes no facts", len(snapshot()["tasks"]), 0)
                expect("validate proposal", command("proposal.validate", {"proposal_id":proposal})[0], 200)
                status, approved = command("proposal.approve", {"proposal_id":proposal})
                expect("UI approval", status, 200)
                grant = approved["data"]["approval"]["id"]
                status, committed = command("changes.commit", {"proposal_id":proposal,"approval_grant":grant})
                expect("approved commit", status, 200)
                expect("commit writes exactly one fact", len(snapshot()["tasks"]), 1)
                expect("consumed grant cannot duplicate commit", command("changes.commit", {"proposal_id":proposal,"approval_grant":grant})[0] >= 400, True)
                expect("replayed approval no duplicated facts", len(snapshot()["tasks"]), 1)

            def plan():
                status, task = command("task.create", {"task":{"title":"Isolated planning task","estimate_minutes":30}})
                expect("create planning task", status, 200)
                task_id = task["data"]["changes"][0]["id"]
                status, _ = command("capacity.create", {"capacity":{"title":"Isolated future capacity","start":"2035-01-01T09:00:00Z","end":"2035-01-01T10:00:00Z","capacity_minutes":60,"locked":False}})
                expect("create capacity", status, 200)
                status, planned = command("plan.propose", {"task_ids":[task_id],"horizon_start":"2035-01-01T00:00:00Z","horizon_end":"2035-01-02T00:00:00Z","move_unlocked":True})
                expect("plan proposed", status, 200)
                expect("planning does not commit allocations", len(snapshot()["allocations"]), 0)
                proposal = planned["data"]["proposal"]["id"]
                status, approved = command("proposal.approve", {"proposal_id":proposal})
                expect("approve plan", status, 200)
                expect("commit plan", command("changes.commit", {"proposal_id":proposal,"approval_grant":approved["data"]["approval"]["id"]})[0], 200)
                expect("one allocation committed", len(snapshot()["allocations"]), 1)

            def automation():
                status, body = command("automation.create", {"automation":{"name":"Isolated bounded automation", "trigger":"manual", "conditions":{}, "actions":[{"kind":"task.create","task":{"title":"Isolated automation fact","estimate_minutes":5}}],"enabled":True,"budget":2}})
                expect("create automation", status, 200)
                rule_id = body["data"]["automation"]["id"]
                before = len(snapshot()["tasks"])
                expect("automation first run", command("automation.run", {"id":rule_id})[0], 200)
                expect("automation second run", command("automation.run", {"id":rule_id})[0], 200)
                expect("automation budget denies third", command("automation.run", {"id":rule_id})[0], 422)
                snap = snapshot()
                expect("automation writes only budgeted facts", len(snap["tasks"]), before+2)
                rule = next(r for r in snap["automation_rules"] if r["id"] == rule_id)
                expect("automation disabled at budget", rule["enabled"], False)
                expect("automation run count", rule["runs"], 2)

            def scheduled_automation():
                status, body = command("automation.create", {"automation":{"name":"Isolated scheduler", "trigger":"schedule", "conditions":{"interval_seconds":60}, "actions":[{"kind":"task.create","task":{"title":"Isolated scheduled fact","estimate_minutes":5}}],"enabled":True,"budget":1}})
                expect("create scheduled automation", status, 200)
                rule_id = body["data"]["automation"]["id"]
                deadline = time.monotonic()+10
                snap = None
                while time.monotonic() < deadline:
                    time.sleep(.2)
                    _, page = query("workspace.snapshot")
                    snap = page["data"]
                    rule = next(r for r in snap["automation_rules"] if r["id"] == rule_id)
                    if rule["runs"]:
                        break
                expect("scheduler ran once", rule["runs"], 1)
                expect("scheduled rule disabled at budget", rule["enabled"], False)
                expect("scheduler created exactly one task", sum((r.get("task") or r)["title"] == "Isolated scheduled fact" for r in snap["tasks"]), 1)

            for name, fn in (("authentication",authentication),("proposal_boundary",proposal_boundary),("planning",plan),("automation",automation),("scheduled_automation",scheduled_automation)):
                scenario(name, fn)
        expect("all owned Core processes exited", all(p.poll() is not None for p, _ in processes), True)
    except Exception:
        checks.append({"name":"fixture", "passed":False,"error":traceback.format_exc()})
    result = {"case":"fresh_api_security_planning_automation", "status":"passed" if all(c["passed"] for c in checks) else "failed", "seconds":round(time.monotonic()-started,2),"checks":checks}
    (root / "checks.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    return result
