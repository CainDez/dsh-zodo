import json
import pathlib
import urllib.error
import urllib.request

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
info = json.loads((ROOT / ".runtime" / "zodo-a2" / "runtime.json").read_text(encoding="utf-8"))
base = f"http://{info['host']}:{info['port']}"


def call(method, path, token, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


ui = info["tokens"]["ui"]
agent = info["tokens"]["agent"]
result = {}
result["agent_before"] = call("GET", "/v1/model", agent)
result["revoke"] = call("POST", "/v1/admin/revoke", ui, {"channel": "agent", "revoked": True})
result["agent_after_revoke"] = call("GET", "/v1/model", agent)
result["ui_after_revoke"] = call("GET", "/v1/model", ui)
result["auth_file"] = json.loads((ROOT / ".runtime" / "zodo-a2" / "auth.json").read_text(encoding="utf-8"))
result["restore"] = call("POST", "/v1/admin/revoke", ui, {"channel": "agent", "revoked": False})
result["agent_after_restore"] = call("GET", "/v1/model", agent)
result["auth_file_after"] = json.loads((ROOT / ".runtime" / "zodo-a2" / "auth.json").read_text(encoding="utf-8"))
(EVID / "a3-revocation.json").write_text(
    json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
)
print(json.dumps({
    "before": result["agent_before"][0],
    "revoke": result["revoke"][0],
    "agent_after_revoke": result["agent_after_revoke"][0],
    "agent_after_revoke_code": result["agent_after_revoke"][1].get("error", {}).get("code"),
    "ui_after_revoke": result["ui_after_revoke"][0],
    "restore": result["restore"][0],
    "agent_after_restore": result["agent_after_restore"][0],
    "auth_file": result["auth_file"],
    "auth_file_after": result["auth_file_after"],
}, ensure_ascii=False, indent=2))
