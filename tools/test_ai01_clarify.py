import json
import os
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
DATA = ROOT / ".runtime" / "ai01-test"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)
if DATA.exists():
    shutil.rmtree(DATA)
DATA.mkdir(parents=True)
env = os.environ.copy()
env["OLLAMA_API_KEY"] = "ollama"
log = (EVID / "ai01-server.log").open("w", encoding="utf-8")
proc = subprocess.Popen([str(Z), "--data-dir", str(DATA), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT, env=env)


def request(info, method, path, body=None, token=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
        headers={"Authorization": f"Bearer {token or info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=90) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def query(info, name):
    return request(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": "ai01", "workspace_id": "ws_local", "query": name, "payload": {},
    })


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
    status, created = request(info, "POST", "/v1/runs", {
        "intent": "capture.clarify",
        "input": {"text": "下周找时间把作品集第二版收尾，可能还要问设计同学要素材"},
        "idempotency_key": "ai01-clarify",
    })
    if status >= 300:
        raise RuntimeError(created)
    run_id = created["run"]["id"]
    run = None
    end = time.time() + 120
    while time.time() < end:
        time.sleep(1)
        get_status, body = request(info, "GET", f"/v1/runs/{run_id}")
        if get_status == 200:
            run = body["run"]
            if run["state"] in ("succeeded", "failed", "cancelled", "interrupted"):
                break
    _, tasks = query(info, "tasks.list")
    result = {
        "create_status": status,
        "run_id": run_id,
        "state": run["state"] if run else None,
        "error": run.get("error") if run else None,
        "output": run.get("output") if run else None,
        "checkpoint": run.get("checkpoint") if run else None,
        "facts_written": len(tasks["data"].get("tasks", [])),
    }
    (EVID / "ai01-clarify.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "state": result["state"],
        "questions": (result["output"] or {}).get("questions"),
        "suggested_kind": (result["output"] or {}).get("suggested_kind"),
        "facts_written": result["facts_written"],
        "error": result["error"],
    }, ensure_ascii=False, indent=2))
finally:
    log.close()
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
