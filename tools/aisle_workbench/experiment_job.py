"""Run a persisted experiment, keeping each repetition and its measured artifacts independent."""

import argparse
import contextlib
import fcntl
from importlib.metadata import version
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from uuid import UUID, uuid4

from authored_scene import load as compile_task
from experiment import encode, recordings, sample, schedule, seeds, sha
from experiment_recording import now
from worker import describe, digest, matches_bound

HERE = Path(__file__).resolve().parent
TERMINAL = {"completed", "failed", "cancelled", "skipped"}


def write(path, value):
    with tempfile.NamedTemporaryFile(
        dir=path.parent, prefix=".write-", delete=False
    ) as tmp:
        tmp.write(json.dumps(value, indent=2, allow_nan=False).encode())
        tmp.flush()
        os.fsync(tmp.fileno())
    os.replace(tmp.name, path)


def read(path):
    return json.loads(path.read_bytes())


def uid(value):
    if str(UUID(value)) != value:
        raise ValueError("Invalid run identifier")
    return value


def trial_path(folder, row):
    return folder / "trials" / uid(row["run_id"])


def freeze_controller(source, target, plan, checksum, task_digest):
    from fleet_worker import companion_names

    keep = companion_names(plan) | {"controller.json"}
    keep.update(
        name
        for name in [
            "controller-request.json",
            "provider-response.json",
            "reply.json",
            "provider-metrics.json",
        ]
        if source.with_name(name).is_file()
    )
    for name in keep:
        shutil.copy2(source.with_name(name), target / name)
    if (
        digest(target / "controller.json") != checksum
        or digest(target / "bound-task.json") != task_digest
    ):
        raise ValueError("Inputs changed while freezing the experiment")


def selected_inputs(source, expected):
    raw = source.read_bytes()
    plan = json.loads(raw)
    checksum = sha(raw)
    if expected and expected != checksum:
        raise ValueError("The selected controller has changed")
    bound = compile_task(source.with_name("bound-task.json"), plan["bound_task_sha256"])
    meta = describe(plan["robot"])
    if meta["model_hash"] != plan["model_hash"] or not matches_bound(bound, plan):
        raise ValueError("Controller, task and selected robot differ")
    return plan, checksum, bound


def preview(config, source, expected, repetition):
    _, _, bound = selected_inputs(source, expected)
    ordered = seeds(config)
    if not 1 <= repetition <= len(ordered):
        raise ValueError("Choose a repetition in this experiment")
    recordings(config, repetition)
    seed = ordered[repetition - 1]
    layout = sample(bound.document, config, seed)
    compile_task(source.with_name("bound-task.json"), bound.sha256, layout)
    return dict(
        seed=seed,
        scene=layout["scene"],
        values=layout["values"],
        placements=layout.get("placements", {}),
        sample_sha256=sha(encode(layout)),
    )


def create(root, config, source, expected=None, *, parent=None):
    plan, checksum, bound = selected_inputs(source, expected)
    job = schedule(config, bound.document, bound.sha256, checksum, plan["model_hash"])
    alias = config.get("worker", "local")
    if alias != "local":
        from experiment_fleet import capabilities

        job["worker"] = capabilities(root.parent, alias, plan)
        if (
            config.get("worker_id", job["worker"]["worker_id"])
            != job["worker"]["worker_id"]
        ):
            raise ValueError("The saved configuration identifies a different worker")
    root.mkdir(parents=True, exist_ok=True)
    dest = root / job["batch_id"]
    with tempfile.TemporaryDirectory(dir=root, prefix=".new-") as tmp:
        stage = Path(tmp) / "batch"
        stage.mkdir()
        for name in ["controller", "requests", "trials"]:
            (stage / name).mkdir()
        freeze_controller(source, stage / "controller", plan, checksum, bound.sha256)
        if parent:
            job["parent"] = parent
        for row in job["repetitions"]:
            if parent:
                row.update(
                    parent_run_id=parent["run_id"],
                    parent_batch_id=parent["batch_id"],
                    variant_id=parent["variant_id"],
                )
            row.update(
                created=now(),
                started=None,
                ended=None,
                nexus_version=job["worker"]["nexus"] if "worker" in job else version("dimforge-nexus3d"),
                asset_revisions=[
                    dict(id=a["id"], revision=a["revision"])
                    for a in bound.document["assets"]
                ],
            )
            prepare_row(stage, job, row)
        job["created"] = now()
        write(stage / "batch.json", job)
        stage.rename(dest)
    return dest


def prepare_row(folder, job, row):
    path = trial_path(folder, row)
    path.mkdir()
    task_file = folder / "controller/bound-task.json"
    try:
        realized = sample(read(task_file), job["configuration"], row["seed"])
        row["sample_sha256"] = sha(encode(realized))
        row["sampled_values"] = realized["values"]
        if "placements" in realized:
            row["placements"] = realized["placements"]
        write(path / "sampled-layout.json", realized)
        compile_task(task_file, row["task_sha256"], realized)
    except (ValueError, TypeError, KeyError, IndexError) as e:
        row.update(status="skipped", reason=str(e), ended=now())
    write(path / "manifest.json", row)


def publish(folder, job, row):
    write(trial_path(folder, row) / "manifest.json", row)
    write(folder / "batch.json", job)


def cancel(folder, identifier):
    job = read(folder / "batch.json")
    uid(identifier)
    row = next(r for r in job["repetitions"] if r["run_id"] == identifier)
    if row["status"] in TERMINAL:
        raise ValueError("This repetition already finished")
    write(folder / "requests" / (identifier + ".cancel"), {"requested": now()})


def retry(folder, identifier):
    uid(identifier)
    job = read(folder / "batch.json")
    row = next(r for r in job["repetitions"] if r["run_id"] == identifier)
    if row["status"] not in TERMINAL:
        raise ValueError("Complete or cancel this repetition before retry")
    token = str(uuid4())
    write(folder / "requests" / (token + ".retry"), dict(parent=identifier))
    return token


def include_retries(folder, job):
    for req in sorted((folder / "requests").glob("*.retry")):
        child = uid(req.stem)
        if any(r["run_id"] == child for r in job["repetitions"]):
            continue
        parent = next(
            r for r in job["repetitions"] if r["run_id"] == read(req)["parent"]
        )
        row = parent.copy()
        for k in (
            "pid",
            "reason",
            "outcome",
            "sampled_values",
            "sample_sha256",
            "channels",
            "worker",
        ):
            row.pop(k, None)
        row.update(
            run_id=child,
            parent_run_id=parent["run_id"],
            status="planned",
            created=now(),
            started=None,
            ended=None,
        )
        path = trial_path(folder, row)
        if path.is_dir() and (path / "manifest.json").is_file():
            row = read(path / "manifest.json")
        else:
            prepare_row(folder, job, row)
        job["repetitions"].append(row)
        publish(folder, job, row)


def finish(folder, job, row, code, *, failure_reason=None):
    path = trial_path(folder, row)
    cancelled = (folder / "requests" / (row["run_id"] + ".cancel")).exists() or row.get(
        "worker", {}
    ).get("state") == "cancelled"
    measured = read(path / "result.json") if (path / "result.json").exists() else None
    if failure_reason is not None:
        status, reason = "failed", failure_reason
        measured = None
    elif measured is not None:
        row["outcome"] = dict(
            passed=bool(measured.get("task_success")),
            result_sha256=digest(path / "result.json"),
            physics_ticks=measured.get("physics_ticks"),
        )
        status = "completed" if row["outcome"]["passed"] else "failed"
        reason = measured.get("error") or (
            "Measured success criteria failed" if status == "failed" else ""
        )
    elif cancelled:
        status, reason = "cancelled", "Operator cancelled this repetition"
    else:
        error = path / "error.json"
        status, reason = "failed", (
            read(error)["message"]
            if error.exists()
            else f"Worker ended before publishing a verdict (exit {code})"
        )
    row.update(status=status, reason=reason, ended=now())
    channels = {}
    for key, name in [
        ("robot_trace", "robot-trace.jsonl"),
        ("agent_history", "agent-history.jsonl"),
    ]:
        file = path / name
        channels[key] = {"requested": row["recording"][key], "present": file.is_file()}
        if file.is_file():
            channels[key].update(
                bytes=file.stat().st_size, sha256=digest(file), file=name
            )
        channels[key]["status"] = (
            "disabled"
            if not row["recording"][key]
            else (
                "missing"
                if not file.is_file()
                else (
                    "partial"
                    if row["status"] in ("cancelled", "skipped")
                    or (key == "robot_trace" and measured is None)
                    else "complete"
                )
            )
        )
        if not row["recording"][key] and file.is_file():
            row.update(status="failed", reason=f"Unexpected optional recording {name}")
        if (
            row["recording"][key]
            and not file.is_file()
            and row["status"] == "completed"
        ):
            row.update(status="failed", reason=f"Missing optional recording {name}")
    row["channels"] = channels
    publish(folder, job, row)


def alive(pid, path):
    # Only attach to the exact worker command recorded for this trial after a supervisor restart.
    if not isinstance(pid, int) or pid <= 0:
        return False
    p = subprocess.run(
        ["ps", "-p", str(pid), "-o", "command="], text=True, capture_output=True
    )
    return p.returncode == 0 and str(path) in p.stdout and "worker.py run" in p.stdout


def execute_row(folder, job, row):
    if "worker" in job:
        from experiment_fleet import execute
        try:
            execute(folder, job, row)
        except (ValueError, KeyError, OSError, TypeError) as exc:
            path = trial_path(folder, row)
            write(path / "error.json", {"message": str(exc)})
            # A partially imported verdict cannot supersede an integrity failure.
            finish(folder, job, row, None, failure_reason=str(exc))
        return
    path = trial_path(folder, row)
    request = folder / "requests" / (row["run_id"] + ".cancel")
    child = None
    guard_path = folder / "requests" / (row["run_id"] + ".lock")
    guard = guard_path.open("a")
    try:
        fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
        locked = True
    except BlockingIOError:
        locked = False
    if not locked:
        guard.close()
        # A child inherited the attempt lock. Wait for its own PID receipt or terminal evidence.
        pid_file = path / "worker-pid.json"
        while not pid_file.exists() and not (path / "error.json").exists():
            time.sleep(0.1)
            with guard_path.open("a") as probe:
                try:
                    fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    continue
                break
        if pid_file.is_file():
            row["pid"] = read(pid_file)["pid"]
        if not alive(row.get("pid"), path):
            finish(folder, job, row, None)
            return
    elif (
        request.exists()
        or (path / "worker-pid.json").is_file()
        or (path / "result.json").is_file()
        or (path / "error.json").is_file()
    ):
        guard.close()
        finish(folder, job, row, None)
        return
    else:
        row.update(status="running", started=row["started"] or now())
        write(
            path / "request.json",
            {"configuration": job["configuration"], "manifest": row},
        )
        publish(folder, job, row)
        args = [
            sys.executable,
            str(HERE / "worker.py"),
            "run",
            "--reserved",
            "--robot",
            row["robot"],
            "--out",
            str(path),
            "--controller",
            str(folder / "controller/controller.json"),
            "--controller-sha256",
            row["controller_sha256"],
            "--seed",
            str(row["seed"]),
            "--experiment-run",
            str(path / "request.json"),
        ]
        if not job["configuration"].get("record_cameras", True):
            args.append("--observations-only")
        with (path / "worker.log").open("a") as log:
            child = subprocess.Popen(
                args,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                pass_fds=(guard.fileno(),),
            )
        guard.close()
        row["pid"] = child.pid
        publish(folder, job, row)
    stop_time = None
    while child.poll() is None if child else alive(row["pid"], path):
        if request.is_file() and (
            stop_time is None or time.monotonic() - stop_time > 5
        ):
            with contextlib.suppress(ProcessLookupError):
                os.killpg(
                    row["pid"], signal.SIGTERM if stop_time is None else signal.SIGKILL
                )
            stop_time = time.monotonic()
        time.sleep(0.2)
    finish(folder, job, row, child.returncode if child else None)


def run(folder):
    with (folder / "supervisor.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        job = read(folder / "batch.json")
        while True:
            include_retries(folder, job)
            row = next(
                (r for r in job["repetitions"] if r["status"] not in TERMINAL), None
            )
            if row is None:
                break
            execute_row(folder, job, row)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    opts = cli.add_subparsers(dest="operation", required=True)
    probe = opts.add_parser("worker")
    probe.add_argument("root", type=Path)
    probe.add_argument("alias")
    probe.add_argument("controller", type=Path)
    new = opts.add_parser("create")
    new.add_argument("root", type=Path)
    new.add_argument("config", type=Path)
    new.add_argument("controller", type=Path)
    new.add_argument("--controller-sha256")
    view = opts.add_parser("preview", help="Validate a sampled repetition without creating a batch")
    view.add_argument("controller", type=Path)
    view.add_argument("repetition", type=int)
    view.add_argument("--controller-sha256", required=True)
    for name in ("run", "cancel", "retry"):
        parser = opts.add_parser(name)
        parser.add_argument("batch", type=Path)
        if name != "run":
            parser.add_argument("run_id")
    a = cli.parse_args()
    if a.operation == "worker":
        from experiment_fleet import capabilities
        print(json.dumps(capabilities(a.root, a.alias, read(a.controller))))
    elif a.operation == "create":
        print(
            create(
                a.root.resolve(),
                read(a.config),
                a.controller.resolve(),
                a.controller_sha256,
            )
        )
    elif a.operation == "preview":
        result = preview(json.load(sys.stdin), a.controller.resolve(), a.controller_sha256, a.repetition)
        print(json.dumps(result))
    elif a.operation == "run":
        run(a.batch.resolve())
    elif a.operation == "cancel":
        cancel(a.batch.resolve(), a.run_id)
    else:
        print(retry(a.batch.resolve(), a.run_id))


if __name__ == "__main__":
    main()
