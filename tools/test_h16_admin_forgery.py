import json
import pathlib
import urllib.error
import urllib.request

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
INFO = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
HOST = "http://127.0.0.1:3181"
EVID = ROOT / "artifacts" / "evidence" / "a3"


def call(url, method="GET", headers=None, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status, dict(response.headers), json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        text = error.read().decode()
        try:
            parsed = json.loads(text)
        except Exception:
            parsed = text
        return error.code, dict(error.headers), parsed


def core(path, token, body=None):
    return call(
        f"http://{INFO['host']}:{INFO['port']}{path}",
        "POST" if body is not None else "GET",
        {"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        body,
    )


no_cookie_health = call(f"{HOST}/zodo-bridge/v1/health")
forged_cookie = "zodo_ui=forged-cookie-value"
forged = call(f"{HOST}/zodo-bridge/v1/rpc", "POST", {"Cookie": forged_cookie, "Content-Type": "application/json", "Origin": HOST}, {
    "operation": "data.backup", "payload": {},
})
evil_origin = call(f"{HOST}/zodo-bridge/v1/session", "POST", {"Origin": "https://evil.invalid", "Content-Type": "application/json"}, {})
agent_admin = core("/v1/data/backup", INFO["tokens"]["agent"], {})
agent_rpc = core("/v1/commands", INFO["tokens"]["agent"], {
    "schema": "zodo.command/1", "request_id": "h16", "idempotency_key": "h16",
    "workspace_id": "ws_local", "operation": "workspace.update", "operation_version": 1,
    "payload": {"settings": None, "policy": None},
})
session = call(f"{HOST}/zodo-bridge/v1/session", "POST", {"Origin": HOST, "Content-Type": "application/json"}, {})
result = {
    "no_cookie_health": {"status": no_cookie_health[0], "ok": no_cookie_health[2].get("ok")},
    "forged_cookie_rpc": {"status": forged[0], "code": forged[2].get("error", {}).get("code")},
    "evil_origin_session": {"status": evil_origin[0], "code": evil_origin[2].get("error", {}).get("code")},
    "agent_admin_backup": {"status": agent_admin[0], "code": agent_admin[2].get("error", {}).get("code")},
    "agent_policy_update": {"status": agent_rpc[0], "code": agent_rpc[2].get("error", {}).get("code")},
    "session_body_has_tokens": any(key in json.dumps(session[2]).lower() for key in ("token", "secret", "api_key")),
    "session_status": session[0],
}
(EVID / "h16-admin-forgery.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(result, ensure_ascii=False, indent=2))
