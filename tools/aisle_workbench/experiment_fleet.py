"""Dispatch a frozen repetition and import the measured trial from its registered worker."""

import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from experiment import encode, sha
from experiment_recording import history, now
from fleet_bridge import import_trial, nodes
from fleet_client import send, submission
from fleet_worker import ACTIVE
from worker import digest


def registered(root, alias):
    registry = nodes(root)
    if alias == "local" or alias not in registry:
        raise ValueError("Select a registered experiment worker")
    return registry[alias]


def capabilities(root, alias, plan):
    reply = send(registered(root, alias), {"op": "hello"})
    if reply.get("experiment_protocol") != 1 or not reply.get("worker_id"):
        raise ValueError("Update this worker to support frozen experiment batches")
    if reply.get("robots", {}).get(plan["robot"]) != plan["model_hash"]:
        raise ValueError("The worker does not have the selected robot model revision")
    return dict(
        alias=alias,
        worker_id=reply["worker_id"],
        backend=reply["backend"],
        nexus=reply["nexus"],
        slots=reply["slots"],
        occupied=len(reply["active"]),
        robot=plan["robot"],
        robot_model_sha256=plan["model_hash"],
        implementation_sha256=reply["worker_sha256"],
    )


def build_request(controller, job, row, trial):
    envelope = {"configuration": job["configuration"], "manifest": dict(row)}
    plan = json.loads(controller.read_bytes())
    if row["recording"]["agent_history"]:
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "agent-history.jsonl"
            history(controller.parent, output, row, plan)
            envelope["retained_agent_history"] = [
                json.loads(line) for line in output.read_text().splitlines()
            ]
    request = submission(controller, "experiment", row["seed"], ident=row["run_id"])
    request["worker_id"] = job["worker"]["worker_id"]
    request["nexus"] = job["worker"]["nexus"]
    request["worker_sha256"] = job["worker"]["implementation_sha256"]
    request["files"]["experiment.json"] = base64.b64encode(encode(envelope)).decode()
    realized = json.loads((trial / "sampled-layout.json").read_bytes())
    request["visual_profile"] = realized["controller"].get("visual_profile", "original")
    return request


def collect(folder, path, row, node, request, status):
    staging = folder / "received"
    staging.mkdir(exist_ok=True)
    entry = dict(
        request=json.dumps(request),
        trial=row["run_id"],
        id=row["run_id"],
        robot=row["robot"],
        node=row["worker"]["alias"],
    )
    received = import_trial(staging, node, entry, status)
    measured = received / "result.json"
    if measured.exists():
        result = json.loads(measured.read_bytes())
        layout = json.loads((received / "sampled-layout.json").read_bytes())
        if (
            result.get("controller_hash") != row["controller_sha256"]
            or result.get("bound_task_sha256") != row["task_sha256"]
            or sha(encode(layout)) != row["sample_sha256"]
        ):
            raise ValueError("Received experiment evidence does not match its inputs")
    # Publish a verified immutable archive under the repetition's existing manifest.
    # Hard links allow a restarted importer to finish without a second full copy.
    for source in sorted(received.rglob("*")):
        if not source.is_file():
            continue
        rel = source.relative_to(received)
        if rel in (Path("manifest.json"), Path("request.json")):
            envelope = json.loads(base64.b64decode(request["files"]["experiment.json"]))
            expected = envelope if rel.name == "request.json" else envelope["manifest"]
            if json.loads(source.read_bytes()) != expected:
                raise ValueError("Worker returned a different submitted manifest")
            continue
        if rel.parts[0] in ("manifest.json", "request.json", "fleet-request.json"):
            raise ValueError(
                "Worker archive collides with the local experiment manifest"
            )
        target = path / rel
        if rel == Path("sampled-layout.json") and target.is_file():
            if sha(encode(json.loads(target.read_bytes()))) != sha(
                encode(json.loads(source.read_bytes()))
            ):
                raise ValueError(
                    "Imported sampling record differs from the local preview"
                )
        elif target.exists():
            if digest(target) != digest(source):
                raise ValueError(f"Local and worker artifacts differ for {rel}")
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            os.link(source, target)


def execute(folder, job, row):
    from experiment_job import finish, publish, read, trial_path, write

    path = trial_path(folder, row)
    cancel = folder / "requests" / (row["run_id"] + ".cancel")
    persisted = path / "fleet-request.json"
    if cancel.is_file() and not persisted.exists():
        finish(folder, job, row, None)
        return
    worker = job["worker"]
    row["worker"] = {
        "alias": worker["alias"],
        "id": worker["worker_id"],
        "lease_id": row["run_id"],
    }
    row.update(status="running", started=row.get("started") or now())
    if not persisted.exists():
        request = build_request(folder / "controller/controller.json", job, row, path)
        envelope = json.loads(base64.b64decode(request["files"]["experiment.json"]))
        write(path / "request.json", envelope)
        write(persisted, request)
    request = read(persisted)
    publish(folder, job, row)
    while True:
        try:
            node = registered(folder.parent.parent, worker["alias"])
            try:
                state = send(node, {"op": "status", "id": row["run_id"]})
            except ValueError as exc:
                if str(exc) != "Unknown simulation lease":
                    raise
                if cancel.is_file():
                    if (
                        send(node, {"op": "hello"}).get("worker_id")
                        != worker["worker_id"]
                    ):
                        raise ValueError(
                            "Cannot confirm cancellation on the original worker"
                        )
                    break
                try:
                    state = send(node, request)
                except ValueError as admission:
                    if str(admission) != "All simulation slots are occupied":
                        raise
                    row["reason"] = "Waiting for a free simulation slot"
                    publish(folder, job, row)
                    time.sleep(2)
                    continue
            if (
                state.get("worker_id") != worker["worker_id"]
                or state["id"] != row["run_id"]
            ):
                raise ValueError("Worker lease identity changed")
            row["worker"]["state"] = state["state"]
            row.pop("reason", None)
            if "progress" in state:
                write(path / "progress.json", state["progress"])
            publish(folder, job, row)
            if state["state"] not in ACTIVE:
                if state["state"] == "lost":
                    raise ValueError(state.get("message") or "Worker lease expired")
                try:
                    collect(folder, path, row, node, request, state)
                except ValueError as exc:
                    if (
                        state["state"] != "cancelled"
                        or str(exc) != "This simulation has no trial artifacts"
                    ):
                        raise
                break
            if cancel.is_file() and state["state"] != "stopping":
                send(node, {"op": "cancel", "id": row["run_id"]})
            time.sleep(0.5)
        except (subprocess.TimeoutExpired, RuntimeError, ConnectionError):
            row["reason"] = "Waiting to reconnect to the registered worker"
            publish(folder, job, row)
            time.sleep(3)
    finish(folder, job, row, None)
