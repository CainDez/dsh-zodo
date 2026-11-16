import subprocess, json, time, pathlib, datetime, sys, shutil

root = pathlib.Path(r"D:\projects\zodo_dsh")
z = str(root / "target" / "release" / "zodo.exe")
data = root / ".runtime" / "evidence-cli-a1"
if data.exists():
    shutil.rmtree(data)
data.mkdir(parents=True)
log_path = root / "artifacts" / "evidence" / "a1" / "cli-server.log"
log = log_path.open("w", encoding="utf-8")
proc = subprocess.Popen(
    [z, "--data-dir", str(data), "serve", "--port", "0"],
    stdout=log,
    stderr=subprocess.STDOUT,
)
try:
    info = None
    for _ in range(120):
        time.sleep(0.1)
        path = data / "runtime.json"
        if path.exists():
            try:
                info = json.loads(path.read_text(encoding="utf-8"))
                break
            except Exception:
                pass
    if not info:
        raise RuntimeError("runtime.json never appeared")
    transcript = [f"instance={info['instance_id']} port={info['port']}"]
    results = {}

    def run(args):
        r = subprocess.run(
            [z, "--data-dir", str(data)] + args,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        transcript.append("$ zodo " + " ".join(args))
        transcript.append("exit=" + str(r.returncode))
        transcript.append(r.stdout.strip())
        if r.stderr.strip():
            transcript.append("stderr: " + r.stderr.strip())
        if r.returncode != 0:
            raise RuntimeError(f"zodo {args} exited {r.returncode}")
        return json.loads(r.stdout) if r.stdout.strip().startswith("{") else r.stdout

    results["capture"] = run(["capture", "证据：明天下午联系印刷店并确认封面"])
    results["task_add"] = run(["task-add", "证据任务：筛选三个案例素材", "--estimate", "45"])
    tasks = run(["task-list"])
    results["task_list"] = tasks
    task = next(row["task"] for row in tasks["data"]["tasks"] if row["task"]["title"] == "证据任务：筛选三个案例素材")
    tid = task["id"]
    now = datetime.datetime.now(datetime.timezone.utc)
    start = (now + datetime.timedelta(hours=1)).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    end = (now + datetime.timedelta(hours=3)).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    payload = {
        "capacity": {
            "title": "证据可用容量",
            "start": start,
            "end": end,
            "capacity_minutes": 120,
            "locked": False,
        }
    }
    payload_path = root / "artifacts" / "evidence" / "a1" / "capacity-payload.json"
    payload_path.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    results["capacity"] = run(["command", "capacity.create", "--payload-file", str(payload_path)])
    plan = run(["plan", "--start", start, "--end", end, "--tasks", tid])
    results["plan"] = plan
    proposal_id = plan["data"]["proposal"]["id"]
    approve = run(["approve", proposal_id])
    results["approve"] = approve
    grant = approve["data"]["approval"]["id"]
    results["commit"] = run(["commit", proposal_id, grant])
    results["progress"] = run(["progress", tid, "--minutes", "10", "--note", "证据进度"])
    results["focus_start"] = run(["focus-start", "--task", tid])
    results["focus_finish"] = run(["focus-finish"])
    results["review"] = run(["review", datetime.date.today().isoformat(), datetime.date.today().isoformat()])
    (root / "artifacts" / "evidence" / "a1" / "cli-vertical.json").write_text(
        json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (root / "artifacts" / "evidence" / "a1" / "cli-vertical.txt").write_text(
        "\n".join(transcript) + "\n", encoding="utf-8"
    )
    print("\n".join(transcript[-12:]))
    print("CLI_EVIDENCE_OK")
finally:
    log.close()
    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
