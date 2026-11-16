"""Frozen AI-output evaluation harness.

This is intentionally offline and deterministic: it scores the frozen predictions in cases.json with
the same constraints the Core applies before a model proposal can be committed. A live-model run is a
separate step; this harness catches schema/invariant regressions without network dependence.
"""
import datetime
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
CASES = ROOT / "evals" / "ai" / "cases.json"
EVID = ROOT / "artifacts" / "evidence" / "a3"
EVID.mkdir(parents=True, exist_ok=True)


def validate(prediction, expect):
    errors = []
    tasks = prediction.get("tasks")
    if not isinstance(tasks, list):
        return ["tasks is not an array"]
    if not (expect["min_tasks"] <= len(tasks) <= expect["max_tasks"]):
        errors.append(f"task count {len(tasks)} outside {expect['min_tasks']}..{expect['max_tasks']}")
    seen = set()
    for index, task in enumerate(tasks):
        if not isinstance(task, dict):
            errors.append(f"task[{index}] is not an object")
            continue
        title = task.get("title")
        if not isinstance(title, str) or not title.strip() or len(title.strip()) > 200:
            errors.append(f"task[{index}] invalid title")
        elif task.get("duplicate_titles_forbidden", False):
            pass
        if isinstance(title, str):
            normalized = title.strip().casefold()
            if expect.get("duplicate_titles_forbidden") and normalized in seen:
                errors.append(f"task[{index}] duplicate title")
            seen.add(normalized)
        estimate = task.get("estimate_minutes")
        if not isinstance(estimate, int) or not (15 <= estimate <= 240):
            errors.append(f"task[{index}] estimate {estimate!r} outside 15..240")
        priority = task.get("priority")
        if not isinstance(priority, int) or not (0 <= priority <= 4):
            errors.append(f"task[{index}] priority invalid")
        due = task.get("target_due")
        if due is not None:
            try:
                datetime.date.fromisoformat(due)
            except Exception:
                errors.append(f"task[{index}] target_due invalid")
    for key in ("assumptions", "questions"):
        values = prediction.get(key, [])
        if not isinstance(values, list):
            errors.append(f"{key} is not an array")
        elif len(values) > expect["max_questions"]:
            errors.append(f"{key} exceeds limit")
    return errors


def main():
    data = json.loads(CASES.read_text(encoding="utf-8"))
    results = []
    failed = 0
    for case in data["cases"]:
        errors = validate(case["prediction"], case["expect"])
        should_fail = bool(case.get("must_fail"))
        passed = bool(errors) if should_fail else not errors
        if not passed:
            failed += 1
        results.append({
            "id": case["id"],
            "must_fail": should_fail,
            "errors": errors,
            "passed": passed,
        })
    privacy_path = ROOT / "evals" / "ai" / "privacy-case.json"
    privacy = json.loads(privacy_path.read_text(encoding="utf-8"))
    privacy_text = json.dumps(privacy["request"], ensure_ascii=False)
    privacy_errors = []
    for forbidden in privacy["forbidden"]:
        if forbidden in privacy_text:
            privacy_errors.append(f"forbidden token present: {forbidden}")
    for required in privacy["required"]:
        if required not in privacy_text:
            privacy_errors.append(f"required context missing: {required}")
    privacy_passed = not privacy_errors
    if not privacy_passed:
        failed += 1
    results.append({
        "id": "ai-privacy-cloud-context",
        "must_fail": False,
        "errors": privacy_errors,
        "passed": privacy_passed,
        "source": "evals/ai/privacy-case.json",
    })
    summary = {
        "schema": "zodo.ai-eval-result/1",
        "frozen_set": "evals/ai/cases.json + evals/ai/privacy-case.json",
        "privacy_passed": privacy_passed,
        "model": data["model"],
        "provider": data["provider"],
        "cases": results,
        "total": len(results),
        "failed": failed,
        "passed": failed == 0,
    }
    (EVID / "ai-eval.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"total": summary["total"], "failed": failed, "passed": summary["passed"]}, indent=2))
    return 0 if summary["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
