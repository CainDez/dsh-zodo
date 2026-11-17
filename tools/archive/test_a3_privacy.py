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
DATA = ROOT / ".runtime" / "a3-privacy-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
# Deliberately break the local endpoint and enable a cloud provider. local_only must not fall back.
(DATA / "model.json").write_text(json.dumps({
    "version": 1,
    "local": {
        "id": "local-ollama",
        "label": "Local broken test route",
        "base_url": "http://127.0.0.1:9/v1",
        "model": "no-such-local-model",
        "api_key_env": "OLLAMA_API_KEY",
        "location": "device",
        "enabled": True,
    },
    "cloud": {
        "id": "deepseek-cloud",
        "label": "DeepSeek cloud test route",
        "base_url": "https://api.deepseek.com/v1",
        "model": "deepseek-chat",
        "api_key_env": "ZODO_TEST_MISSING_DEEPSEEK_KEY",
        "location": "cloud",
        "enabled": True,
    },
}, ensure_ascii=False, indent=2), encoding="utf-8")
env = os.environ.copy()
env["OLLAMA_API_KEY"] = "ollama"
log = (EVID / "a3-privacy-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen(
    [str(Z), "--data-dir", str(DATA), "serve", "--port", "0"],
    stdout=log,
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

    def call(method, path, body=None):
        data = None if body is None else json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            base + path,
            data=data,
            method=method,
            headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=60) as response:
                return response.status, json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read().decode("utf-8"))

    def command(operation, payload):
        return call("POST", "/v1/commands", {
            "schema": "zodo.command/1",
            "request_id": f"priv-{uuid.uuid4()}",
            "idempotency_key": f"priv-{uuid.uuid4()}",
            "workspace_id": "ws_local",
            "operation": operation,
            "operation_version": 1,
            "payload": payload,
        })

    def query(name, payload):
        return call("POST", "/v1/queries", {
            "schema": "zodo.query/1",
            "request_id": f"priv-{uuid.uuid4()}",
            "workspace_id": "ws_local",
            "query": name,
            "payload": payload,
        })

    command("goal.create", {"goal": {"title": "隐私路由测试目标", "success_criteria": "验证无静默回退"}})
    _, goals = query("goals.list", {})
    goal_id = next(g["id"] for g in goals["data"]["goals"] if g["title"] == "隐私路由测试目标")

    def run_and_wait(label):
        status, created = call("POST", "/v1/runs", {
            "intent": "goal.decompose",
            "input": {"goal_id": goal_id, "horizon_end": "2026-11-15T23:59:59Z"},
            "idempotency_key": label,
        })
        if status not in (200, 202):
            return {"create_status": status, "create_body": created}
        run_id = created["run"]["id"]
        run = None
        deadline = time.time() + 45
        while time.time() < deadline:
            qs, body = call("GET", f"/v1/runs/{run_id}")
            if qs == 200:
                run = body["run"]
                if run["state"] in ("succeeded", "failed", "cancelled"):
                    break
            time.sleep(0.4)
        return {"create_status": status, "run_id": run_id, "run": run}

    result = {"local_only_broken_local": run_and_wait("privacy-local-only")}
    command("workspace.update", {
        "settings": None,
        "policy": {
            "model_route": "cloud_allowed",
            "private_notes": "device_only",
            "allowed_cloud_providers": ["deepseek-cloud"],
            "diagnostic_capture": False,
        },
    })
    result["cloud_allowed_private_device_only"] = run_and_wait("privacy-cloud-denied")
    command("workspace.update", {
        "settings": None,
        "policy": {
            "model_route": "cloud_allowed",
            "private_notes": "cloud_allowed",
            "allowed_cloud_providers": ["deepseek-cloud"],
            "diagnostic_capture": False,
        },
    })
    result["cloud_allowed_missing_credential"] = run_and_wait("privacy-cloud-missing-key")
    (EVID / "a3-privacy.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    summary = {}
    for key, value in result.items():
        run = value.get("run")
        summary[key] = {
            "create_status": value.get("create_status"),
            "state": run.get("state") if run else None,
            "error": run.get("error") if run else None,
            "output": run.get("output") if run else None,
        }
    print(json.dumps(summary, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
