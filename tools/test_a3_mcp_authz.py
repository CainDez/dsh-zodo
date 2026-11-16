import json
import pathlib
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
EVID = ROOT / "artifacts" / "evidence" / "a3"
info = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
base = f"http://{info['host']}:{info['port']}"


def call(path, token, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        base + path,
        data=data,
        method="POST" if body is not None else "GET",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def command(token, operation, payload):
    return call("/v1/commands", token, {
        "schema": "zodo.command/1",
        "request_id": f"a3-{uuid.uuid4()}",
        "idempotency_key": f"a3-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "operation": operation,
        "operation_version": 1,
        "payload": payload,
    })


def query(token, name, payload):
    return call("/v1/queries", token, {
        "schema": "zodo.query/1",
        "request_id": f"a3-{uuid.uuid4()}",
        "workspace_id": "ws_local",
        "query": name,
        "payload": payload,
    })


mcp = info["tokens"]["mcp"]
ui = info["tokens"]["ui"]
qs, proposals = query(ui, "proposals.list", {})
validated = [p for p in proposals["data"]["proposals"] if p["status"] == "validated"]
proposal_id = validated[0]["id"] if validated else "missing"
result = {
    "proposal_id": proposal_id,
    "mcp_snapshot_private": query(mcp, "workspace.snapshot", {}),
    "mcp_approve": command(mcp, "proposal.approve", {"proposal_id": proposal_id}),
    "mcp_commit": command(mcp, "changes.commit", {"proposal_id": proposal_id, "approval_grant": "forged"}),
    "mcp_proposal_create": command(mcp, "proposal.create", {
        "intent": "MCP 权限测试：只允许提案",
        "operations": [{"operation": "task.create", "payload": {"task": {"title": "MCP 权限测试任务"}}}],
    }),
    "cross_workspace": call("/v1/queries", ui, {
        "schema": "zodo.query/1",
        "request_id": f"a3-{uuid.uuid4()}",
        "workspace_id": "ws_other",
        "query": "workspace.get",
        "payload": {},
    }),
}
# Redact any token-looking values just in case; the request bodies here contain no tokens.
(EVID / "a3-mcp-authz.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({
    "proposal_id": proposal_id,
    "snapshot_status": result["mcp_snapshot_private"][0],
    "snapshot_code": result["mcp_snapshot_private"][1].get("error", {}).get("code"),
    "approve_status": result["mcp_approve"][0],
    "approve_code": result["mcp_approve"][1].get("error", {}).get("code"),
    "commit_status": result["mcp_commit"][0],
    "commit_code": result["mcp_commit"][1].get("error", {}).get("code"),
    "proposal_create_status": result["mcp_proposal_create"][0],
    "cross_workspace_status": result["cross_workspace"][0],
    "cross_workspace_code": result["cross_workspace"][1].get("error", {}).get("code"),
}, ensure_ascii=False, indent=2))
