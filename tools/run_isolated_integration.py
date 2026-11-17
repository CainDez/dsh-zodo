"""Run only reviewed legacy cases on fresh, task-owned data. Never run live scripts.

python tools/run_isolated_integration.py [--binary target/release/zodo.exe]
Evidence and temporary Core data stay in tests/integration/runs/<unique>/.
This is a reviewed-source adapter, not a sandbox for arbitrary Python scripts.
"""
import argparse
import ast
import contextlib
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import traceback
import urllib.parse
import urllib.request

sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[1]
CASES = {
    "test_fnd11_events.py": ("fnd11-events.json", {"first_count": 2, "second_count": 1, "duplicate_count": 0, "future_resync_required": True, "limited_has_more": True, "all_queries_ok": True}),
    "test_h09_receipt_replay.py": ("h09-receipt-replay.json", {"first_status": 200, "replay_status": 200, "replay_replayed": True, "mismatch_status": 409, "count_after_first": 1, "count_after_replay": 1, "count_after_mismatch": 1}),
    "test_h10_restart_cursor.py": ("h10-restart-cursor.json", {"restart_status": 200, "new_events": 1, "resync_required": False, "same_workspace": True, "same_instance": False}),
    "test_pln11_calendar.py": ("a4-pln11-calendar.json", {"first_status": 200, "first_events": 2, "second_status": 200, "second_events": 3, "replace_status": 200, "replace_events": 1, "bad_status": 422, "events_after_bad": 1}),
    "test_dat04_import.py": ("a4-dat04-import.json", {"preflight_status": 200, "first_status": 200, "replay_status": 200, "replay_flag": True, "mismatch_status": 409, "tasks_after": 1}),
    "test_dat04_batch.py": ("dat04-batch-import.json", {"batch_status": 200, "replay_status": 200, "target_d_tasks_after_conflict": 0}),
    "test_dat08_encrypted_sync.py": ("dat08-encrypted-sync.json", {"export_status": 200, "plaintext_absent": True, "import_status": 200, "target_tasks": 1, "wrong_status": 403, "wrong_code": "PERMISSION_DENIED", "wrong_tasks": 0}),
    "test_a6_sync.py": ("a6-swap.json", {"import_b_status": 200, "import_a_status": 200, "a_tasks_after_roundtrip": 2, "b_tasks_after_first_sync": 2, "conflict_status": 409, "retry_status": 200, "b_retry_shared_title": "A 修改后的标题", "revoked_import_status": 403}),
    "test_a4_restore.py": ("a4-restore.json", {"before_count": 2, "restore_exit": 0, "after_count": 1, "query_status": 200}),
    "test_ai11_cloud_stub.py": ("ai11-cloud-stub.json", {"goal_status": 200, "model_status": 200, "cloud_requests": 1, "private_note_in_cloud_request": False, "stub_received_goal": True, "task_count": 3, "approved_status": 200, "committed_status": 200}),
    "test_pln16_calendar_write.py": ("pln16-calendar-write.json", {"first_status": 200, "duplicate_status": 422, "second_status": 200, "external_writes": 2}),
}
REVIEWED_SHA256 = {
    "test_fnd11_events.py": "73ea3fdb301db9339d5ebaf5036d514bbbb4994e314c4fa07333dbb10563f948",
    "test_h09_receipt_replay.py": "64407103b124ef7b98590c9b04b3420d4867b23e407b4fe2e0b3dd00aeb3da89",
    "test_h10_restart_cursor.py": "65a28d5f7e1931b8ca8348799ea0036a7473c6aec135a782aa5f8c6e2149e579",
    "test_pln11_calendar.py": "b438e1a585211b45fefc343b5ba7b456dafc73c773311445dcec9c258210b9c5",
    "test_dat04_import.py": "359e796bf1ea2adec77df96d639f346436c57d61992346477c3f2534e5221671",
    "test_dat04_batch.py": "fb190dfb45c6d6b9281e62a8bd830db1917f4ad043195be0a6f5e2a21f37299f",
    "test_dat08_encrypted_sync.py": "1fcf462ba91cc20ee5300913d107e0efced597303f08cafe7f9a822890396c1a",
    "test_a6_sync.py": "24e3ceb5ba80c3324cc71f573b28d249942e53592094d63a19b2ba31366b96d9",
    "test_a4_restore.py": "3aa23b56eadedf5354d2bf0238f19b322802407b08c3b5b28baa19aa60ea1b32",
    # Re-reviewed: current cloud_assisted policy; synthetic loopback stub/dummy keys only;
    # ROOT/PORT/local endpoint are relocated and owned_processes retains its guards.
    "test_ai11_cloud_stub.py": "d425a71511dd4e0c5bb5fde2cc6128d8835e95039ca6d991503e9e3fd37c7e8d",
    "test_pln16_calendar_write.py": "c9fff852b394d592c06556464ca86963659d5bfb26e525c15ff35411f2156070"
}

# Historical sources live under tools/archive and are never admitted for execution.
SKIPPED = {
    "archive/test_a3_cancel.py": "archive/live runtime dependency, preexisting goal and real model",
    "archive/test_a3_mcp_authz.py": "archive/live runtime dependency; covered by fresh API suite",
    "archive/test_a3_revocation.py": "archive/live auth mutation; covered by fresh API suite",
    "archive/test_a3_notification.py": "archive/live runtime and real Windows toast",
    "archive/test_a4_scheduler.py": "archive/live runtime; covered by fresh API suite",
    "archive/test_a4_import.py": "exports archive/live data; replaced by synthetic dat04 tests",
    "archive/test_h16_admin_forgery.py": "archive/live runtime and existing bridge port 3181",
    "archive/test_restore_preflight.py": "reads frozen archive backup files",
    "archive/test_fnd06_reconcile.py": "real Windows toast has no disable/mock flag",
    "archive/test_pln14_notification_retry.py": "restores real Windows toast script and delivers notification",
    "archive/test_a3_model_run.py": "real local model on port 11434",
    "archive/test_a3_recovery.py": "real local model on port 11434",
    "archive/test_h21_cancel_race.py": "real local model; later coverage needs dedicated deterministic stub",
    "archive/test_ai01_clarify.py": "real local model; later coverage needs dedicated deterministic stub",
    "archive/test_a3_privacy.py": "external cloud URL and real credential inheritance risk; do not run unchanged",
}


def safe_env(root):
    # Deliberate allowlist: no inherited provider secrets, Core settings, or proxy.
    env = {k: v for k, v in os.environ.items() if k.upper() in {
        "SYSTEMROOT", "WINDIR", "COMSPEC", "PATH", "PATHEXT", "SYSTEMDRIVE",
        "PROCESSOR_ARCHITECTURE", "NUMBER_OF_PROCESSORS",
    }}
    for key in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"):
        dest = root / "environment" / key.lower()
        dest.mkdir(parents=True, exist_ok=True)
        env[key] = str(dest)
    env.update(PYTHONDONTWRITEBYTECODE="1", PYTHONIOENCODING="utf-8", NO_PROXY="127.0.0.1,localhost")
    return env


class Relocate(ast.NodeTransformer):
    def __init__(self, root, binary, reject_url):
        self.root, self.binary, self.reject_url = root, binary, reject_url

    def visit_Assign(self, node):
        self.generic_visit(node)
        names = [t.id for t in node.targets if isinstance(t, ast.Name)]
        if "ROOT" in names:
            node.value = ast.Call(func=ast.Attribute(value=ast.Name(id="pathlib", ctx=ast.Load()), attr="Path", ctx=ast.Load()), args=[ast.Constant(str(self.root))], keywords=[])
        elif "Z" in names:
            node.value = ast.Call(func=ast.Attribute(value=ast.Name(id="pathlib", ctx=ast.Load()), attr="Path", ctx=ast.Load()), args=[ast.Constant(str(self.binary))], keywords=[])
        elif "PORT" in names:
            node.value = ast.Constant(0)
        if "server" in names and isinstance(node.value, ast.Call):
            return [node, ast.parse("PORT = server.server_address[1]").body[0]]
        return node

    def visit_Constant(self, node):
        if node.value == "http://127.0.0.1:11434/v1":
            return ast.copy_location(ast.Constant(self.reject_url), node)
        return node


@contextlib.contextmanager
def owned_processes(root, binary, reject_url):
    original = subprocess.Popen
    original_urlopen = urllib.request.urlopen
    original_server_init = http.server.ThreadingHTTPServer.__init__
    procs, ports, servers = [], set(), []

    def server_init(server, *args, **kwargs):
        original_server_init(server, *args, **kwargs)
        ports.add(server.server_address[1])
        servers.append(server)

    class GuardedPopen(original):
        def __init__(self, args, *positional, **kwargs):
            argv = list(map(str, args))
            if Path(argv[0]).resolve() != binary or "--data-dir" not in argv:
                raise RuntimeError("refusing unowned executable or implicit Core data")
            data = Path(argv[argv.index("--data-dir") + 1]).resolve()
            if not data.is_relative_to(root) or data == root:
                raise RuntimeError("refusing Core data outside unique fixture tree")
            if "serve" in argv:
                if argv != [str(binary), "--data-dir", str(data), "serve", "--port", "0"]:
                    raise RuntimeError("Core must use reviewed serve argv and an ephemeral port")
            elif "restore" in argv:
                if len(argv) != 5 or argv[1:4] != ["--data-dir", str(data), "restore"]:
                    raise RuntimeError("restore must use reviewed argv")
                if not Path(argv[-1]).resolve().is_relative_to(root):
                    raise RuntimeError("restore backup must belong to this fixture tree")
            else:
                raise RuntimeError("unreviewed Core command")
            env = safe_env(root)
            requested = kwargs.get("env", {})
            for key in ("OLLAMA_API_KEY", "DEEPSEEK_API_KEY"):
                if key in requested:
                    if requested[key] not in ("ollama", "stub-cloud-key"):
                        raise RuntimeError("only dummy test credentials allowed")
                    env[key] = requested[key]
            env["ZODO_LOCAL_MODEL_URL"] = reject_url
            calendar = requested.get("ZODO_CALENDAR_WRITE_URL")
            if calendar:
                parsed = urllib.parse.urlparse(calendar)
                if parsed.hostname != "127.0.0.1" or parsed.port not in ports:
                    raise RuntimeError("calendar endpoint must be owned stub")
                env["ZODO_CALENDAR_WRITE_URL"] = calendar
            kwargs["env"] = env
            kwargs["cwd"] = str(root)
            super().__init__(argv, *positional, **kwargs)
            procs.append((self, data))

    def urlopen(request, *args, **kwargs):
        url = request.full_url if hasattr(request, "full_url") else str(request)
        parsed = urllib.parse.urlparse(url)
        allowed = set(ports)
        for proc, data in procs:
            runtime = data / "runtime.json"
            if proc.poll() is None and runtime.exists():
                info = json.loads(runtime.read_text(encoding="utf-8"))
                if info.get("pid") == proc.pid and info.get("host") in ("127.0.0.1", "localhost"):
                    allowed.add(info["port"])
        if parsed.hostname not in ("127.0.0.1", "localhost") or parsed.port not in allowed:
            raise RuntimeError("refusing HTTP request to non-owned service")
        return original_urlopen(request, *args, **kwargs)

    subprocess.Popen = GuardedPopen
    urllib.request.urlopen = urlopen
    http.server.ThreadingHTTPServer.__init__ = server_init
    try:
        yield procs
    finally:
        subprocess.Popen = original
        urllib.request.urlopen = original_urlopen
        http.server.ThreadingHTTPServer.__init__ = original_server_init
        for proc, _ in procs:
            if proc.poll() is None:
                proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
        for server in servers:
            server.shutdown()
            server.server_close()


class RejectModel(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.send_response(503)
        self.end_headers()
        self.wfile.write(b'{"error":"model disabled by regression runner"}')


def run_case(name, root, binary, reject_url):
    source_path = REPO / "tools" / name
    source = source_path.read_text(encoding="utf-8")
    if hashlib.sha256(source.encode()).hexdigest() != REVIEWED_SHA256[name]:
        raise RuntimeError(f"Source changed since safety review: {name}; review before updating admission hash")
    root.mkdir()
    for stage in ("a1", "a3", "a4", "a6"):
        (root / "artifacts" / "evidence" / stage).mkdir(parents=True)
    output = io.StringIO()
    started = time.monotonic()
    failures = []
    try:
        tree = Relocate(root, binary, reject_url).visit(ast.parse(source))
        ast.fix_missing_locations(tree)
        with owned_processes(root, binary, reject_url) as processes, contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            exec(compile(tree, str(source_path), "exec"), {"__name__": "__main__", "__file__": str(source_path)})
        if any(proc.poll() is None for proc, _ in processes):
            raise AssertionError("owned Core survived cleanup")
        filename, expectations = CASES[name]
        result_path = next(root.rglob(filename))
        result = json.loads(result_path.read_text(encoding="utf-8"))
        for key, expected in expectations.items():
            if result.get(key) != expected:
                failures.append(f"{key}: expected {expected!r}, got {result.get(key)!r}")
        if name == "test_ai11_cloud_stub.py":
            if result.get("run", {}).get("state") != "succeeded" or len(result["dependencies"]) != 2:
                failures.append("AI run must succeed and commit two dependencies")
        if name == "test_dat04_batch.py":
            if result["tasks_after_batch"] != ["批次 A", "批次 B"] or result["conflict_status"] < 400 or not result["replay"].get("replayed"):
                failures.append("batch merge/replay/conflict invariant violated")
        if name in ("test_h09_receipt_replay.py", "test_dat04_import.py"):
            first = result.get("first_revision", result.get("revision_after_first"))
            replay = result.get("replay_revision", result.get("revision_after_replay"))
            if first != replay:
                failures.append("idempotent replay changed revision")
    except Exception:
        failures.append(traceback.format_exc())
    (root / "output.log").write_text(output.getvalue(), encoding="utf-8")
    return {"case": name, "status": "failed" if failures else "passed", "seconds": round(time.monotonic()-started, 2), "failures": failures, "source_sha256": hashlib.sha256(source.encode()).hexdigest(), "fixture": str(root.relative_to(REPO))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/zodo.exe")
    parser.add_argument("--case", action="append", choices=sorted(CASES))
    parser.add_argument("--api-only", action="store_true", help="run only fresh authorization/planning/automation checks")
    args = parser.parse_args()
    if args.case and args.api_only:
        parser.error("--case and --api-only are mutually exclusive")
    binary = (REPO / args.binary).resolve()
    if not binary.is_file():
        parser.error(f"Core binary missing: {binary}")
    parent = REPO / "tests" / "integration" / "runs"
    parent.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="isolated-", dir=parent)).resolve()
    # No secrets are carried into legacy script environment, even before Popen.
    original_env = dict(os.environ)
    isolated_env = safe_env(root)
    os.environ.clear()
    os.environ.update(isolated_env)
    import threading
    reject = http.server.ThreadingHTTPServer(("127.0.0.1", 0), RejectModel)
    thread = threading.Thread(target=reject.serve_forever, daemon=True)
    thread.start()
    reject_url = f"http://127.0.0.1:{reject.server_address[1]}/v1"
    results = []
    try:
        for name in ([] if args.api_only else args.case or CASES):
            result = run_case(name, root / name.removesuffix(".py"), binary, reject_url)
            results.append(result)
            print(json.dumps(result, ensure_ascii=False), flush=True)
        if not args.case:
            import isolated_api_suite
            result = isolated_api_suite.run(root / "fresh_api", binary, reject_url, owned_processes)
            results.append(result)
            print(json.dumps(result, ensure_ascii=False), flush=True)
    finally:
        reject.shutdown()
        reject.server_close()
        os.environ.clear()
        os.environ.update(original_env)
    summary = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "results": results, "skipped": SKIPPED, "passed": sum(r["status"] == "passed" for r in results), "failed": sum(r["status"] == "failed" for r in results)}
    (root / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"SUMMARY {summary['passed']} passed, {summary['failed']} failed; {root / 'summary.json'}")
    return int(summary["failed"] > 0)


if __name__ == "__main__":
    sys.exit(main())
