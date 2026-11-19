"""Exercise two reviewed Zodo Core builds across one owned synthetic SQLite profile.

This is not a Desktop, plugin lifecycle, or browser test. No user profile,
external origin, stored credentials, or third-party dependency is accepted.
"""
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
from contextlib import closing, contextmanager


def digest(path):
    with path.open("rb") as src:
        return hashlib.file_digest(src, "sha256").hexdigest()


def exercise(old, new, phase):
    assert old != new and digest(old) != digest(new)
    with tempfile.TemporaryDirectory(prefix="zodo-upgrade-fixture-") as td:
        root = Path(td)
        data = root / 'data'
        home = root / 'home'
        home.mkdir()
        env = {k: v for k, v in os.environ.items() if k.upper() in ("SYSTEMROOT", "WINDIR")}
        env["RUST_LOG"] = "off"
        active_pid = None
        env.update({k: str(home) for k in ('HOME','USERPROFILE','LOCALAPPDATA','APPDATA','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_CACHE_HOME','TEMP','TMP','TMPDIR')})

        def runtime_info():
            info = json.loads((data / "runtime.json").read_text(encoding="utf-8"))
            return info["pid"], info["host"], info["port"]

        def invoke(binary, *args):
            pid, host, port = runtime_info()
            assert pid == active_pid and host == "127.0.0.1"
            assert type(port) is int and 0 < port < 65536
            out = subprocess.run([str(binary), '--data-dir', str(data), *args], env=env, cwd=root, capture_output=True, text=True, encoding='utf-8', timeout=15)
            if out.returncode != 0:
                raise RuntimeError("Core CLI exited unsuccessfully")
            result = json.loads(out.stdout)
            if 'data' not in result or result.get('error'):
                raise RuntimeError("Core CLI returned an unexpected result")
            return result['data']

        @contextmanager
        def running(binary):
            nonlocal active_pid
            assert active_pid is None
            # Core startup stdout includes synthetic loopback credentials; discard it.
            with open(os.devnull, "wb") as output:
                args = [str(binary), '--data-dir', str(data), 'serve', '--host', '127.0.0.1', '--port', '0']
                proc = subprocess.Popen(args, env=env, cwd=root, stdout=output, stderr=subprocess.STDOUT,
                                        creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == 'nt' else 0)
                try:
                    until = time.monotonic() + 15
                    while time.monotonic() < until:
                        if proc.poll() is not None:
                            raise RuntimeError("Core exited during startup")
                        try:
                            pid, host, port = runtime_info()
                        except (OSError, ValueError, KeyError):
                            time.sleep(0.05)
                            continue
                        if pid == proc.pid and host == "127.0.0.1" and type(port) is int and 0 < port < 65536:
                            active_pid = pid
                            break
                        time.sleep(0.05)
                    else:
                        raise RuntimeError("Expected isolated Core did not bind")
                    yield
                finally:
                    active_pid = None
                    if proc.poll() is None:
                        try:
                            proc.send_signal(signal.CTRL_BREAK_EVENT if os.name == 'nt' else signal.SIGINT)
                            proc.wait(timeout=5)
                        except (OSError, subprocess.TimeoutExpired):
                            proc.kill()
                            proc.wait(timeout=5)
        def integrity():
            with closing(sqlite3.connect((data / 'zodo.sqlite3').as_uri() + '?mode=ro', uri=True)) as db:
                assert db.execute('pragma quick_check').fetchall() == [('ok',)]
        def cmd(binary, name, payload):
            path = root / 'payload.json'
            path.write_text(json.dumps(payload), encoding='utf-8')
            return invoke(binary, 'command', name, '--payload-file', str(path), '--key', secrets.token_hex(16))
        def snap(binary):
            return invoke(binary, 'query', 'workspace.snapshot')
        def day_at(hm):
            return f'{day}T{hm}:00+08:00'
        day = dt.datetime.now(dt.timezone(dt.timedelta(hours=8))).date() + dt.timedelta(days=2)
        while day.weekday() >= 5:
            day += dt.timedelta(days=1)

        phase[0] = "old-startup"
        with running(old):
            phase[0] = "old-seed"
            for title in ('upgrade locked', 'upgrade movable', 'upgrade committed'):
                cmd(old, 'task.create', {'task': {'title': title, 'estimate_minutes': 30}})
            cmd(old, 'capacity.create', {'capacity': {'title': 'upgrade capacity', 'start': day_at('09:00'), 'end': day_at('12:00'), 'capacity_minutes': 180}})
            cmd(old, 'commitment.create', {'commitment': {'title': 'upgrade fixed', 'start': day_at('08:00'), 'end': day_at('08:30'), 'locked': True}})
            seeded = snap(old)
            assert seeded['workspace']['settings']['timezone'] == 'Asia/Shanghai'
            cap = next(x['id'] for x in seeded['capacity_blocks'] if x['title'] == 'upgrade capacity')
            ids = {x['title'].split(' ')[1]: x['id'] for x in seeded['tasks'] if x['title'].startswith('upgrade ')}
            for label, start, end in (('locked','09:00','09:30'), ('movable','09:30','10:00')):
                cmd(old, 'allocation.create', {'allocation': {'task_id': ids[label], 'capacity_block_id': cap, 'start': day_at(start), 'end': day_at(end), 'minutes': 30, 'locked': label == 'locked'}})
            plan = cmd(old, 'plan.propose', {'task_ids': [ids['committed']], 'horizon_start': day_at('10:00'), 'horizon_end': day_at('12:00'), 'move_unlocked': False})
            proposal = plan['proposal']
            assert proposal['status'] == 'validated' and proposal['operations']
            grant = cmd(old, 'proposal.approve', {'proposal_id': proposal['id']})['approval']
            cmd(old, 'changes.commit', {'proposal_id': proposal['id'], 'approval_grant': grant['id']})
            baseline = snap(old)
            assert len(baseline['allocations']) == 3 and len(baseline['commitments']) == 1
            assert baseline['commitments'][0]['locked'] is True
            assert next(x for x in baseline['allocations'] if x['task_id'] == ids['locked'])['locked'] is True
            assert next(x for x in baseline['allocations'] if x['task_id'] == ids['movable'])['locked'] is False
            assert next(x for x in baseline['proposals'] if x['id'] == proposal['id'])['status'] == 'applied'
        phase[0] = "old-integrity"
        integrity()
        phase[0] = "upgrade-startup"
        with running(new):
            phase[0] = "upgrade-read"
            state = snap(new)
            for key in ('tasks', 'allocations', 'commitments', 'capacity_blocks', 'calendar_events', 'focus_sessions', 'proposals'):
                assert state[key] == baseline[key], f'old/new differs in {key}'
            previous_workspace = {k: v for k, v in baseline['workspace'].items() if k != 'policy'}
            current_workspace = {k: v for k, v in state['workspace'].items() if k != 'policy'}
            assert previous_workspace == current_workspace
            assert state['workspace']['policy'] == {
                'model_access': {'goals': False, 'projects': False, 'captures': False},
                'diagnostic_capture': False,
            }
            phase[0] = "upgrade-write"
            cmd(new, 'task.create', {'task': {'title': 'upgrade new', 'estimate_minutes': 15}})
            target = next(x for x in snap(new)['tasks'] if x['title'] == 'upgrade new')
            ph = cmd(new, 'proposal.create', {'intent':'upgrade synthetic strict', 'operations':[{'operation':'allocation.schedule','payload': {'allocation': {'task_id':target['id'],'expected_task_version':target['version'],'capacity_block_id':cap,'start':day_at('11:00'),'end':day_at('11:15')}}}]})['proposal']
            assert ph['status'] == 'validated'
            gr = cmd(new, 'proposal.approve', {'proposal_id':ph['id']})['approval']
            assert snap(new)['allocations'] == baseline['allocations']
            cmd(new, 'changes.commit', {'proposal_id':ph['id'],'approval_grant':gr['id']})
            after = snap(new)
            assert len(after['allocations']) == 4 and len([x for x in after['allocations'] if x['task_id'] == target['id']]) == 1
            for x in baseline['allocations']:
                assert x in after['allocations']
        phase[0] = "upgrade-integrity"
        integrity()
        phase[0] = "restart"
        with running(new):
            end_state = snap(new)
            end_state.pop('now', None)
            after.pop('now', None)
            assert end_state == after
        phase[0] = "cleanup"
    assert not root.exists(), "owned temporary directory was not removed"
    print(json.dumps({
        "status": "passed",
        "core_profile_upgrade": True,
        "desktop_plugin_lifecycle": False,
        "old_core_sha256": digest(old),
        "new_core_sha256": digest(new),
        "prior_allocations": 3,
        "new_allocations": 1,
        "new_restart_snapshot_preserved": True,
        "temporary_data_removed": True,
    }))


def main():
    # The runner captures subprocess output; never surface a traceback, a runtime
    # descriptor, paths, raw app responses, the seed rows, or loopback tokens.
    phase = ["input"]
    try:
        if len(sys.argv) != 3 or not __debug__:
            raise ValueError()
        old, new = (Path(value).resolve(strict=True) for value in sys.argv[1:])
        exercise(old, new, phase)
    except Exception:
        print(json.dumps({"status": "failed", "phase": phase[0], "core_profile_upgrade": False}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
