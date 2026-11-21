#!/usr/bin/env python3
"""Run the real DSH Web renderer on disposable Linux x64 state, not a live host.

Use an already installed, verified @deepseek-ai/dsh 0.2.2-rc.5 npm prefix and
playwright-core 1.64.1 (the existing tests/browser/ci-deps pin). No downloads.
Provide an explicit trusted Chrome/Chromium path; Chromium sandbox is required.
"""
import argparse
from contextlib import contextmanager
import hashlib
from http.cookiejar import CookieJar
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import select
import signal
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import (HTTPCookieProcessor, HTTPRedirectHandler, ProxyHandler,
                            Request, build_opener)
from uuid import uuid4
from zipfile import ZipFile

HERE = Path(__file__).resolve().parent
DSH_VERSION = "0.2.2-rc.5"


def check(ok):
    if not ok:
        raise ValueError("Acceptance invariant not met")


def file(path):
    path = path.resolve(strict=True)
    check(path.is_file())
    return path


def stop_group(proc):
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=6)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait(timeout=3)


def run_group(argv, root, env, timeout, private_input=None, stdout=subprocess.DEVNULL):
    with subprocess.Popen(argv, cwd=root, env=env, stderr=subprocess.DEVNULL, stdout=stdout,
                          stdin=subprocess.PIPE if private_input else subprocess.DEVNULL,
                          start_new_session=True) as proc:
        try:
            proc.communicate(input=private_input, timeout=timeout)
            check(proc.returncode == 0)
        finally:
            if proc.poll() is None:
                stop_group(proc)


def stop_core(data, dsh_home, digest):
    state = data / "runtime.json"
    if not state.is_file():
        return
    pid = json.loads(state.read_text())["pid"]
    check(type(pid) is int and pid > 1)
    proc = Path("/proc") / str(pid)
    # pidfd protects against PID reuse before signaling. Verify ownership first.
    try:
        fd = os.pidfd_open(pid)
    except ProcessLookupError:
        return
    try:
        try:
            exe = (proc / "exe").resolve(strict=True)
            check(exe.is_relative_to(dsh_home))
            check((proc / "cwd").resolve(strict=True) == data)
            check(hashlib.sha256(exe.read_bytes()).hexdigest() == digest)
            signal.pidfd_send_signal(fd, signal.SIGTERM)
        except (FileNotFoundError, ProcessLookupError):
            return
        if not select.select([fd], [], [], 6)[0]:
            try:
                signal.pidfd_send_signal(fd, signal.SIGKILL)
            except ProcessLookupError:
                return
            check(select.select([fd], [], [], 3)[0])
    finally:
        os.close(fd)


@contextmanager
def official_host(dsh, root, env):
    with tempfile.TemporaryFile(dir=root) as log:
        host = subprocess.Popen(dsh + ["web", "--no-open", "--host", "127.0.0.1", "--port", "0"],
                                cwd=root, env=env, stdin=subprocess.DEVNULL,
                                stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            end = time.monotonic() + 20
            while time.monotonic() < end:
                out = os.pread(log.fileno(), 1_000_000, 0)
                match = re.search(rb"dsh web: (http://127\.0\.0\.1:[0-9]+/\?token=[A-Za-z0-9_-]+)(?=\r?\n)", out)
                if match:
                    break
                check(host.poll() is None and len(out) < 1_000_000)
                time.sleep(.15)
            else:
                raise TimeoutError()
            yield match[1]
        finally:
            stop_group(host)


BRIDGE = "/zodo-bridge/v1"
MAX_RESPONSE = 2_000_000


def bridge_session(launch):
    # Use only this private Host, never environment proxies or cookies. A
    # bootstrap redirect can only remain on the original exact loopback origin.
    url = launch.decode("ascii")
    parts = urlsplit(url)
    check(parts.scheme == "http" and parts.hostname == "127.0.0.1" and
          parts.username is None and 0 < parts.port <= 65535)
    origin = f"http://127.0.0.1:{parts.port}"

    class SameOriginRedirect(HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            actual = urlsplit(newurl)
            check(not urlsplit(req.full_url).path.startswith(BRIDGE + "/") and
                  actual.scheme == "http" and actual.netloc == parts.netloc)
            return super().redirect_request(req, fp, code, msg, headers, newurl)

    opener = build_opener(ProxyHandler({}), HTTPCookieProcessor(CookieJar()), SameOriginRedirect())
    with opener.open(url, timeout=12) as response:
        check(response.status == 200)
        check(len(response.read(MAX_RESPONSE + 1)) <= MAX_RESPONSE)

    def request(path, data=None, *, absent=False):
        check(path in {"/health", "/session", "/rpc"})
        req = Request(origin + BRIDGE + path, headers={
            "Origin": origin, "Content-Type": "application/json",
        }, data=None if data is None else json.dumps(data).encode("utf-8"))
        try:
            with opener.open(req, timeout=12) as response:
                check(not absent and response.status == 200)
                raw = response.read(MAX_RESPONSE + 1)
                check(len(raw) <= MAX_RESPONSE)
                body = json.loads(raw)
                check(body["ok"] is True)
                return body
        except HTTPError as error:
            with error:
                check(absent and error.code == 404)
                return None
    return request


def durable_snapshot(launch, data):
    request = bridge_session(launch)
    core = request("/health")["core"]
    check(core["data_dir"] == str(data) and
          isinstance(core["instance_id"], str) and bool(core["instance_id"]))
    session = request("/session", {})
    check("session_token" not in session)  # browser-side cookie only
    result = request("/rpc", {"kind": "query", "envelope": {
        "schema": "zodo.query/1", "workspace_id": "ws_local", "request_id": str(uuid4()),
        "query": "workspace.snapshot", "payload": {},
    }})
    facts = result["result"]["data"]
    # All other snapshot fields are durable workspace facts or projections thereof.
    del facts["now"]
    revision = facts["workspace"]["revision"]
    check(type(revision) is int and revision > 0)
    rows = [row for row in facts["tasks"] if row["title"] == "Official Web acceptance task"]
    check(len(rows) == 1 and bool(rows[0]["id"]))
    return core["instance_id"], facts


def run(args):
    phase = "local-inputs"
    try:
        check(platform.system() == "Linux" and platform.machine() == "x86_64")
        modules = args.dsh_prefix.resolve(strict=True) / "node_modules"
        cli = file(modules / ".bin" / "dsh")
        check(cli.is_relative_to((modules / "@deepseek-ai/dsh").resolve(strict=True)))
        check(json.loads((cli.parent.parent / "package.json").read_text())["version"] == DSH_VERSION)
        chrome = file(args.chrome)
        pw = args.playwright.resolve(strict=True)
        check(json.loads((pw / "package.json").read_text())["version"] == "1.64.1")
        file(pw / "index.mjs")
        node = file(args.node)
        spec = importlib.util.spec_from_file_location(
            "zodo_package", HERE.parents[1] / "scripts/verify-plugin-package.py")
        verifier = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(verifier)
        phase = "verify-and-unpack"
        with tempfile.TemporaryDirectory(prefix="zodo-real-web-") as tmp:
            root = Path(tmp).resolve()
            # Copy and verify the same private ZIP that we unpack.
            archive = root / "verified.zip"
            archive.write_bytes(file(args.zip).read_bytes())
            digest = verifier.verify(archive, file(args.core), target="linux-x64")
            unpacked = root / "plugin"
            unpacked.mkdir()
            with ZipFile(archive) as package:
                package.extractall(unpacked)  # verifier checked all names, modes and bytes
            (unpacked / verifier.LINUX_CORE).chmod(0o755)
            clean = {key: root / name for key, name in {
                "HOME": "home", "TMPDIR": "tmp", "DSH_HOME": "dsh",
                "XDG_CONFIG_HOME": "cfg", "XDG_DATA_HOME": "data",
                "XDG_CACHE_HOME": "cache", "XDG_STATE_HOME": "state",
                "XDG_RUNTIME_DIR": "runtime", "ZODO_DATA_DIR": "core",
                "npm_config_store_dir": "pnpm-store",
            }.items()}
            for directory in clean.values():
                directory.mkdir(mode=0o700)
            # Do not inherit auth, profiles, proxies, debug hooks or browser settings.
            env = {key: str(value) for key, value in clean.items()}
            env.update(PATH=os.environ["PATH"], LANG="C.UTF-8", CI="1",
                       npm_config_offline="true", npm_config_ignore_scripts="true",
                       DSH_TELEMETRY_DISABLED="1", PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD="1")
            phase = "node-version"
            subprocess.run([str(node), "-e", (
                "const [major, minor] = process.versions.node.split('.').map(Number);"
                "if (major < 24 || (major === 24 && minor < 9)) process.exit(1)"
            )], cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                check=True, timeout=5)
            dsh = [str(node), str(cli)]
            try:
                phase = "offline-plugin-install"
                run_group(dsh + ["plugin", "--profile", "web", "add", "file:" + str(unpacked)],
                          root, env, timeout=45)
                phase = "official-loopback-launch"
                with official_host(dsh, root, env) as launch:
                    phase = "browser-temp"
                    # Chromium creates a SingletonSocket inside TMPDIR. The
                    # nested runner path can exceed Linux's Unix socket limit.
                    with tempfile.TemporaryDirectory(prefix="zodo-pw-", dir="/tmp") as browser_tmp:
                        check(len(os.fsencode(browser_tmp)) <= 40)
                        browser_env = {**env, "TMPDIR": browser_tmp}
                        phase = "sandboxed-browser"
                        # The launcher secret travels through a private pipe only.
                        run_group([str(node), str(HERE / "official-web-acceptance.mjs"), str(chrome), str(pw)],
                                  root, browser_env, timeout=100, private_input=launch, stdout=None)
                    phase = "official-before-restart-snapshot"
                    before_instance, before_facts = durable_snapshot(launch, clean["ZODO_DATA_DIR"])
                phase = "official-first-core-stop"
                stop_core(clean["ZODO_DATA_DIR"], clean["DSH_HOME"], digest)

                phase = "official-restart-launch"
                with official_host(dsh, root, env) as launch:
                    phase = "official-after-restart-snapshot"
                    after_instance, after_facts = durable_snapshot(launch, clean["ZODO_DATA_DIR"])
                    check(after_instance != before_instance and after_facts == before_facts)
                phase = "official-restarted-core-stop"
                stop_core(clean["ZODO_DATA_DIR"], clean["DSH_HOME"], digest)
                print("PASS official-host-core-restart-persisted-facts", flush=True)

                phase = "official-plugin-remove"
                run_group(dsh + ["plugin", "--profile", "web", "remove", "@zodo/harness-plugin"],
                          root, env, timeout=30)
                manifest = clean["DSH_HOME"] / "profiles" / "web" / "package.json"
                check("@zodo/harness-plugin" not in json.loads(manifest.read_text()).get("dependencies", {}))
                phase = "official-after-uninstall-launch"
                with official_host(dsh, root, env) as launch:
                    phase = "official-after-uninstall-bridge-absent"
                    bridge_session(launch)("/health", absent=True)
                print("PASS official-uninstall-bridge-absent", flush=True)
            finally:
                stop_core(clean["ZODO_DATA_DIR"], clean["DSH_HOME"], digest)
        return 0
    except (Exception, KeyboardInterrupt):
        # A launcher/error object may contain the temporary login token.
        print(f"FAIL {phase}", flush=True)
        return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("dsh-prefix", "zip", "core", "node", "chrome", "playwright"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    for name, value in vars(args).items():
        if "://" in value:
            parser.error("Only local paths are supported")
        setattr(args, name, Path(value))
    return run(args)


if __name__ == "__main__":
    def on_terminate(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, on_terminate)
    raise SystemExit(main())
