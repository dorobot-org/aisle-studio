"""Nano's Dora graph contract and launcher (simulation only)."""

from __future__ import annotations

import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import time

from nano_validation import digest, write_json

MODES = ("left", "right", "both")
FRAME_NS = 100_000_000
SCHEMA = "nano.joint_position.v1"


def request_metadata(request_id, frames, model):
    if not 0 <= request_id <= 3 * (frames + 1):
        raise ValueError("Request outside run")
    episode, offset = divmod(request_id, frames + 1)
    op = "finish" if episode == 3 else ("reset" if offset == 0 else "frame")
    frame = offset - 1 if op == "frame" else -1
    return dict(
        request_id=request_id,
        episode=episode,
        frame=frame,
        op=op,
        mode=MODES[episode] if episode < 3 else "done",
        sim_time_ns=episode * (frames * FRAME_NS + 10_000_000)
        + (10_000_000 + frame * FRAME_NS if op == "frame" else 0),
        schema=SCHEMA,
        units="rad",
        joint_order=hashlib.sha256("\n".join(model["names"]).encode()).hexdigest(),
    )


class CommandGate:
    """Reject malformed, stale, out-of-limit and too-fast commands before physics."""

    def __init__(self, model, frames):
        self.model, self.frames, self.next_id = model, frames, 0
        self.previous = [0.0] * 17

    def accept(self, values, metadata):
        expected = request_metadata(self.next_id, self.frames, self.model)
        if any(metadata.get(k) != v for k, v in expected.items()):
            raise ValueError(f"Unexpected command identity/schema: expected {expected}")
        if len(values) != 17 or not all(math.isfinite(v) for v in values):
            raise ValueError("Command must contain 17 finite joint positions")
        for i, (name, value) in enumerate(zip(self.model["names"], values)):
            low, high = self.model["limits"][name]
            if not low <= value <= high:
                raise ValueError(f"Joint limit violation: {name}")
            inactive = (
                expected["mode"] in MODES[:2]
                and name in self.model[MODES[1 - MODES.index(expected["mode"])]]
            )
            if (
                expected["op"] != "frame" or name in self.model["held"] or inactive
            ) and abs(value) > 1e-7:
                raise ValueError(f"Nonzero held/reset joint: {name}")
            speed = self.model["velocities"][name]
            if (
                expected["op"] == "frame"
                and abs(value - self.previous[i]) > speed * 0.1 + 1e-6
            ):
                raise ValueError(f"Joint velocity limit violation: {name}")
        self.previous = list(values)
        self.next_id += 1
        return expected


class FrameBarrier:
    """One acknowledged turn; never coalesce, overwrite, or skip a sensor frame."""

    def __init__(self, required, request_id):
        self.required, self.request_id, self.seen = set(required), request_id, set()

    def add(self, channel, request_id):
        if (
            request_id != self.request_id
            or channel not in self.required
            or channel in self.seen
        ):
            raise ValueError(
                f"Duplicate, unknown or out-of-order frame: {channel}/{request_id}"
            )
        self.seen.add(channel)
        return self.seen == self.required


def validate_graph(graph):
    nodes = {n["id"]: n for n in graph["nodes"]}
    if len(nodes) != 5 or set(nodes) != {
        "nano_task",
        "nano_trajectory",
        "nano_limits",
        "nano_sim",
        "nano_recorder",
    }:
        raise ValueError("Expected five distinct Nano Dora nodes")
    expected = {
        "nano_task": {"ready": "nano_sim/ready", "ack": "nano_recorder/ack"},
        "nano_trajectory": {"request": "nano_task/request"},
        "nano_limits": {"command": "nano_trajectory/joint_cmd"},
        "nano_sim": {"command": "nano_limits/joint_cmd_safe"},
        "nano_recorder": {
            "ready": "nano_sim/ready",
            "proposed": "nano_trajectory/joint_cmd",
            "approved": "nano_limits/joint_cmd_safe",
            **{
                k: "nano_sim/" + k
                for k in (
                    "reset_done",
                    "joint_state",
                    "head",
                    "left_wrist",
                    "right_wrist",
                    "objects",
                    "gripper_applied",
                    "applied",
                    "frame_done",
                    "finished",
                )
            },
        },
    }
    for name, inputs in expected.items():
        actual = {k: v["source"] for k, v in nodes[name]["inputs"].items()}
        if actual != inputs:
            raise ValueError(f"Unexpected control/recording edges: {name}")
        for inp in nodes[name]["inputs"].values():
            if (
                inp.get("queue_policy") != "backpressure"
                or inp.get("queue_size", 0) < 1
            ):
                raise ValueError("Every Nano edge requires bounded backpressure")


def collect_dora(args, model):
    import yaml

    cli = shutil.which(getattr(args, "dora", None) or "dora")
    if cli is None:
        raise ValueError(
            "Install Dora: uv tool install --with dora-rs==1.0.1 dora-rs-cli==1.0.1"
        )
    version = subprocess.check_output([cli, "--version"], text=True).strip()
    if version != "dora-cli 1.0.1" or importlib.metadata.version("dora-rs") != "1.0.1":
        raise ValueError("This workflow requires Dora CLI and Python API 1.0.1")
    if args.camera_rig != "nano":
        raise ValueError(
            "Dora collection requires --camera-rig nano (all three cameras)"
        )
    args.output.mkdir(parents=True)
    folder = Path(__file__).resolve().parent
    graph = yaml.safe_load((folder / "nano_dora.yaml").read_text())
    validate_graph(graph)
    config = {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()}
    config["runtime"] = dict(
        cli_version=version,
        python_api_version="1.0.1",
        cli_sha256=digest(Path(cli)),
        implementation="dora-rs",
        provenance="matching release wheels; not source-pin benchmark attestation",
    )
    snapshot = args.output / "node-source"
    snapshot.mkdir()
    for name in (
        "nano_dora.py",
        "nano_dora_node.py",
        "nano_dora_sim.py",
        "nano_dora.yaml",
        "nano_cameras.py",
        "nano_workspace.py",
        "nano_validation.py",
    ):
        shutil.copy2(folder / name, snapshot / name)
    config["sources"] = {p.name: digest(p) for p in snapshot.iterdir()}
    write_json(args.output / "dora-config.json", config)
    for node in graph["nodes"]:
        role = node["id"].removeprefix("nano_")
        node["path"] = sys.executable
        node["args"] = shlex.join(["-u", str(snapshot / "nano_dora_node.py"), role])
        node["env"] = {
            "NANO_DORA_CONFIG": str(args.output / "dora-config.json"),
            "PYTHONPATH": os.pathsep.join([str(snapshot), str(args.aisle / "src")]),
            "PYTHONUNBUFFERED": "1",
        }
    graph_path = args.output / "dataflow.yaml"
    graph_path.write_text(yaml.safe_dump(graph, sort_keys=False))
    log_path = args.output / "dora.log"
    with log_path.open("w") as log:
        proc = subprocess.Popen(
            [cli, "run", str(graph_path), "--log-format", "json"],
            cwd=args.output,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            deadline = time.monotonic() + getattr(args, "timeout", 600)
            while proc.poll() is None:
                failures = sorted(args.output.glob("failure-*.json"))
                if failures:
                    raise RuntimeError(f"Dora node failed: {failures[0].read_text()}")
                if time.monotonic() > deadline:
                    raise subprocess.TimeoutExpired(proc.args, args.timeout)
                time.sleep(0.2)
            code = proc.returncode
        except (subprocess.TimeoutExpired, KeyboardInterrupt, RuntimeError):
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
            raise
    if code:
        raise RuntimeError(f"Dora dataflow failed ({code}); see {log_path}")
    source = args.output / "aisle-run"
    proof = json.loads((source / "dora-recording.json").read_text())
    if (
        not proof["complete"]
        or proof["frames"] != 3 * args.frames
        or len(set(proof["node_pids"].values())) != 5
    ):
        raise ValueError(
            "Incomplete capture or nodes did not run in five separate processes"
        )
    contract = json.loads((source / "contract.json").read_text())
    outcomes = [
        json.loads(line)
        for line in (source / "episodes.jsonl").read_text().splitlines()
    ]
    return source, contract, outcomes
