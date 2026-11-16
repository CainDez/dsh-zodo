import json
import pathlib
import shutil
import subprocess
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(r"D:\projects\zodo_dsh")
Z = ROOT / "target" / "release" / "zodo.exe"
EVID = ROOT / "artifacts" / "evidence" / "a6"
EVID.mkdir(parents=True, exist_ok=True)
PASS = "correct horse battery staple"


def start(name):
    data = ROOT / ".runtime" / f"dat08-{name}"
    if data.exists():
        shutil.rmtree(data)
    data.mkdir(parents=True)
    log = (EVID / f"dat08-{name}.log").open("w", encoding="utf-8")
    proc = subprocess.Popen([str(Z), "--data-dir", str(data), "serve", "--port", "0"], stdout=log, stderr=subprocess.STDOUT)
    info = None
    for _ in range(150):
        time.sleep(0.1)
        path = data / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                if info.get("pid") == proc.pid:
                    break
            except Exception:
                pass
    if not info:
        raise RuntimeError(f"{name} did not start")
    return proc, log, info


def call(info, method, path, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode()
    req = urllib.request.Request(
        f"http://{info['host']}:{info['port']}" + path, data=data, method=method,
        headers={"Authorization": f"Bearer {info['tokens']['ui']}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, json.loads(response.read().decode())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode())


def task_list(info):
    _, body = call(info, "POST", "/v1/queries", {
        "schema": "zodo.query/1", "request_id": "dat08", "workspace_id": "ws_local",
        "query": "tasks.list", "payload": {"filter": {"limit": 100}},
    })
    return body["data"]["tasks"]


source = target = wrong = None
logs = []
try:
    source, log_s, source_info = start("source")
    logs.append(log_s)
    call(source_info, "POST", "/v1/commands", {
        "schema": "zodo.command/1", "request_id": "dat08", "idempotency_key": f"dat08-{uuid.uuid4()}",
        "workspace_id": "ws_local", "operation": "task.create", "operation_version": 1,
        "payload": {"task": {"title": "加密同步任务", "estimate_minutes": 15}},
    })
    status, envelope = call(source_info, "POST", "/v1/sync/export/encrypted", {"passphrase": PASS})
    ciphertext = envelope.get("ciphertext_hex", "")

    target, log_t, target_info = start("target")
    logs.append(log_t)
    import_status, imported = call(target_info, "POST", "/v1/sync/import", {
        "bundle": envelope, "passphrase": PASS,
    })
    target_tasks = task_list(target_info)

    wrong, log_w, wrong_info = start("wrong")
    logs.append(log_w)
    wrong_status, wrong_body = call(wrong_info, "POST", "/v1/sync/import", {
        "bundle": envelope, "passphrase": "wrong passphrase value",
    })
    wrong_tasks = task_list(wrong_info)
    result = {
        "export_status": status,
        "schema": envelope.get("schema"),
        "kdf": envelope.get("kdf"),
        "contains_credentials": envelope.get("contains_credentials"),
        "plaintext_absent": "加密同步任务" not in json.dumps(envelope, ensure_ascii=False) and "加密同步任务" not in ciphertext,
        "ciphertext_len": len(ciphertext),
        "import_status": import_status,
        "imported": imported,
        "target_tasks": len(target_tasks),
        "wrong_status": wrong_status,
        "wrong_code": wrong_body.get("error", {}).get("code"),
        "wrong_tasks": len(wrong_tasks),
    }
    (EVID / "dat08-encrypted-sync.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "schema": result["schema"],
        "plaintext_absent": result["plaintext_absent"],
        "import_status": import_status,
        "target_tasks": result["target_tasks"],
        "wrong_status": wrong_status,
        "wrong_code": result["wrong_code"],
        "wrong_tasks": result["wrong_tasks"],
    }, ensure_ascii=False, indent=2))
finally:
    for proc, log in ((source, logs[0] if logs else None), (target, logs[1] if len(logs) > 1 else None), (wrong, logs[2] if len(logs) > 2 else None)):
        if log:
            log.close()
        if proc:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except Exception:
                proc.kill()
