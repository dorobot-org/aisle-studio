"""Five independent Dora processes. Invoke with the role from nano_dora.yaml."""

from __future__ import annotations

from collections import defaultdict
import io
import hashlib
import json
import os
from pathlib import Path
import sys

import numpy as np
import pyarrow as pa
from dora import Node

from nano_dora import CommandGate, FrameBarrier, request_metadata
from nano_validation import digest, load_model, motion_target, write_json

CAMERAS = ("head", "left_wrist", "right_wrist")


def events(node):
    while True:
        event = node.next(timeout=120)
        if event is None or event["type"] in ("STOP", "ERROR"):
            raise RuntimeError(f"Dora stopped before this node completed: {event}")
        if event["type"] == "INPUT":
            # Dora adds a wall-clock datetime; keep it in the audit, but never
            # forward it as user metadata or confuse it with simulation time.
            event["dora_timestamp"] = str(event["metadata"].get("timestamp", ""))
            event["metadata"] = {
                k: v for k, v in event["metadata"].items() if k != "timestamp"
            }
            yield event


def send(node, topic, value, meta, role):
    meta = dict(meta, **{role + "_pid": os.getpid()})
    if isinstance(value, dict):
        value = pa.array([json.dumps(value, allow_nan=False)], type=pa.string())
    elif not isinstance(value, pa.Array):
        value = pa.array(value, type=pa.float32())
    node.send_output(topic, value, meta)


def unpack(event):
    return json.loads(event["value"][0].as_py())


def task(node, config, model):
    request_id, started = 0, False
    final = 3 * (config["frames"] + 1)
    for event in events(node):
        if event["id"] == "ready":
            if started:
                raise ValueError("Repeated simulator ready")
            started = True
        elif event["id"] == "ack":
            if not started or event["metadata"]["request_id"] != request_id:
                raise ValueError("Unexpected recording acknowledgement")
            if request_id == final:
                return
            request_id += 1
        else:
            raise ValueError("Unexpected task input")
        meta = request_metadata(request_id, config["frames"], model)
        instruction = f"Software validation: {meta['mode']} arm joint motion; fixed base; simulation only"
        send(node, "request", {"instruction": instruction}, meta, "task")


def trajectory(node, config, model):
    next_id = 0
    for event in events(node):
        meta = dict(event["metadata"])
        expected = request_metadata(next_id, config["frames"], model)
        if any(meta.get(k) != v for k, v in expected.items()):
            raise ValueError("Trajectory received an unexpected request")
        meta["instruction"] = unpack(event)["instruction"]
        values = [0.0] * 17
        if meta["op"] == "frame":
            from nano_workspace import workspace_target

            fn = workspace_target if config["workspace"] else motion_target
            values = fn(model, meta["mode"], meta["frame"] / (config["frames"] - 1))
        send(node, "joint_cmd", values, meta, "trajectory")
        next_id += 1
        if meta["op"] == "finish":
            return


def limits(node, config, model):
    gate = CommandGate(model, config["frames"])
    for event in events(node):
        meta = dict(event["metadata"])
        if event["value"].type != pa.float32():
            raise ValueError("Command Arrow type must be float32")
        values = event["value"].to_pylist()
        try:
            gate.accept(values, meta)
        except ValueError as error:
            write_json(
                Path(config["output"]) / "guard-rejection.json",
                dict(
                    error=str(error), request_id=meta.get("request_id"), forwarded=False
                ),
            )
            raise
        send(node, "joint_cmd_safe", event["value"], meta, "limits")
        if meta["op"] == "finish":
            return


def sim(node, config, model):
    from nano_dora_sim import NanoSimulation

    simulation = NanoSimulation(config, model)
    gate = CommandGate(model, config["frames"])
    send(node, "ready", simulation.info(), {"sim_time_ns": 0}, "sim")
    for event in events(node):
        meta = dict(event["metadata"])
        if "limits_pid" not in meta or event["value"].type != pa.float32():
            raise ValueError("Simulator requires a checked float32 command")
        target = event["value"].to_numpy(zero_copy_only=False)
        gate.accept(target, meta)
        if simulation.stamp != meta["sim_time_ns"]:
            raise ValueError("Simulator clock disagrees with command turn")
        if meta["op"] in ("reset", "finish"):
            simulation.reset()
            if meta["op"] == "finish":
                send(node, "finished", {"physics_steps": simulation.tick}, meta, "sim")
                return
            send(node, "reset_done", [1.0], meta, "sim")
            # A real physics tick separates reset and task activation, so the
            # offline exporter never has to invent an order at equal timestamps.
            simulation.step()
        else:
            for topic, value, stamp in simulation.frame(target):
                if topic in CAMERAS:
                    buffer = io.BytesIO()
                    np.savez_compressed(buffer, **value)
                    value = pa.array(np.frombuffer(buffer.getbuffer(), dtype=np.uint8))
                send(node, topic, value, dict(meta, sim_time_ns=stamp), "sim")


def outcome(episode, rows, model):
    mode = ("left", "right", "both")[episode]
    peak = max(r["tracking_error"] for r in rows)
    excess = max(r["limit_excess"] for r in rows)
    excursion = np.ptp(np.asarray([r["after"] for r in rows]), axis=0)
    moving = {
        side: float(max(excursion[model["names"].index(n)] for n in model[side]))
        for side in ("left", "right")
    }
    active = ("left", "right") if mode == "both" else (mode,)
    ok = peak < 0.08 and excess < 0.03 and all(moving[s] > 0.03 for s in active)
    return dict(
        episode=episode,
        env_id=0,
        status="success" if ok else "fail",
        task_family="nano-studio-motion-check",
        mode=mode,
        peak_tracking_error_rad=peak,
        max_joint_limit_excess_rad=excess,
        measured_arm_excursion_rad=moving,
        acceptance="finite state, tracking error <0.08 rad, limit excess <0.03 rad, each commanded arm excursion >0.03 rad; software fixture only",
    )


def recorder(node, config, model):
    from aisle.harness.trace_recorder import TRACE_SCHEMA

    source = Path(config["output"]) / "aisle-run"
    (source / "traces").mkdir(parents=True)
    rows, measured, outcomes = defaultdict(list), [], []
    request_id, frame_count = 0, 0
    barrier, pending, info = None, {}, None
    pids = {"recorder": os.getpid()}
    audit = (source / "dora-events.jsonl").open("w")

    def record(topic, stamp, data=None, text=None):
        rows[topic].append(
            dict(
                sim_time_ns=stamp, env_id=0, seq=len(rows[topic]), data=data, text=text
            )
        )

    def flush():
        for topic, values in rows.items():
            with pa.OSFile(str(source / "traces" / (topic + ".arrow")), "wb") as sink:
                with pa.ipc.new_stream(sink, TRACE_SCHEMA) as writer:
                    writer.write_table(
                        pa.Table.from_pylist(values, schema=TRACE_SCHEMA)
                    )

    try:
        for event in events(node):
            topic, meta = event["id"], dict(event["metadata"])
            for role in ("task", "trajectory", "limits", "sim"):
                if role + "_pid" in meta:
                    pid = meta[role + "_pid"]
                    if role in pids and pids[role] != pid:
                        raise ValueError("Node identity changed mid-run")
                    pids[role] = pid
            payload = (
                event["value"][0].as_py().encode()
                if event["value"].type == pa.string()
                else event["value"].to_numpy(zero_copy_only=False).tobytes()
            )
            audit.write(
                json.dumps(
                    dict(
                        topic=topic,
                        metadata=meta,
                        dora_timestamp=event["dora_timestamp"],
                        arrow_type=str(event["value"].type),
                        elements=len(event["value"]),
                        payload_sha256=hashlib.sha256(payload).hexdigest(),
                    )
                )
                + "\n"
            )
            audit.flush()
            if topic == "ready":
                if info is not None:
                    raise ValueError("Repeated manifest")
                info = unpack(event)
                write_json(source / "manifest.json", info["manifest"])
                write_json(source / "contract.json", info["contract"])
                continue
            expected = request_metadata(request_id, config["frames"], model)
            if meta.get("request_id") != request_id:
                raise ValueError("Recorder received a stale/future turn")
            if barrier is None:
                required = {"proposed", "approved"}
                required |= (
                    {"reset_done"}
                    if expected["op"] == "reset"
                    else {"finished"}
                    if expected["op"] == "finish"
                    else {
                        "joint_state",
                        "objects",
                        "gripper_applied",
                        "applied",
                        "frame_done",
                        *CAMERAS,
                    }
                )
                barrier = FrameBarrier(required, request_id)
            complete = barrier.add(topic, request_id)
            pending[topic] = event
            if topic in CAMERAS:
                if info is None or event["value"].type != pa.uint8():
                    raise ValueError(
                        "Camera payload arrived without a manifest or as a non-byte array"
                    )
                raw = event["value"].to_numpy(zero_copy_only=False).tobytes()
                spec = info["manifest"]["cameras"][topic]
                with np.load(io.BytesIO(raw), allow_pickle=False) as camera:
                    w, h = spec["resolution"]
                    if (
                        int(camera["sim_time_ns"]) != meta["sim_time_ns"]
                        or camera["rgb"].shape != (h, w, 3)
                        or camera["depth"].shape != (h, w)
                        or camera["seg"].shape != (h, w)
                    ):
                        raise ValueError(
                            "Camera timestamp/dimensions disagree with Dora envelope"
                        )
                directory = source / "traces/frames" / spec["source_directory"]
                directory.mkdir(parents=True, exist_ok=True)
                with (directory / f"{meta['sim_time_ns']:020d}.npz").open("xb") as file:
                    file.write(raw)
            if not complete:
                continue
            if info is None:
                raise ValueError("Frame completed before manifest")
            approved = pending["approved"]["value"].to_pylist()
            if pending["proposed"]["value"].to_pylist() != approved:
                raise ValueError(
                    "Guard altered command without a declared transformation"
                )
            start = expected["sim_time_ns"]
            if expected["op"] in ("reset", "finish"):
                record("nano_reset__reset_done", start, [1.0])
                if expected["op"] == "finish":
                    flush()
                    (source / "episodes.jsonl").write_text(
                        "".join(json.dumps(o) + "\n" for o in outcomes)
                    )
                    audit.flush()
                    write_json(
                        source / "dora-recording.json",
                        dict(
                            complete=True,
                            frames=frame_count,
                            camera_frames={n: frame_count for n in CAMERAS},
                            acknowledged_turns=request_id + 1,
                            node_pids=pids,
                            transport="dora-rs",
                            physics_steps=unpack(pending["finished"])["physics_steps"],
                            events_sha256=digest(source / "dora-events.jsonl"),
                        ),
                    )
                else:
                    measured = []
            else:
                timestamps = {
                    "gripper_applied": start + 10_000_000,
                    **{
                        n: start + 20_000_000
                        for n in ("joint_state", "objects", *CAMERAS)
                    },
                    "applied": start + 30_000_000,
                    "frame_done": start + 100_000_000,
                }
                for name, stamp in timestamps.items():
                    if pending[name]["metadata"]["sim_time_ns"] != stamp:
                        raise ValueError(f"Unsynchronized capture: {name}")
                if pending["applied"]["value"].to_pylist() != approved:
                    raise ValueError(
                        "Simulator applied a command different from guard output"
                    )
                grippers = [
                    approved[model["names"].index(s + "_j6_gripper")]
                    for s in ("left", "right")
                ]
                if pending["gripper_applied"]["value"].to_pylist() != grippers:
                    raise ValueError("Simulator applied different gripper targets")
                if expected["frame"] == 0:
                    record(
                        "nano_task__target_request",
                        start,
                        text=json.dumps(
                            {
                                "instruction": pending["approved"]["metadata"][
                                    "instruction"
                                ]
                            }
                        ),
                    )
                record(
                    "nano_sim__gripper_cmd_safe",
                    timestamps["gripper_applied"],
                    grippers,
                )
                record("nano_sim__joint_cmd_safe", timestamps["applied"], approved)
                record(
                    "nano_state__joint_state",
                    timestamps["joint_state"],
                    pending["joint_state"]["value"].to_pylist(),
                )
                record("nano_camera__rgb_overhead", timestamps["head"])
                for name in CAMERAS:
                    record(
                        "nano_camera__" + name,
                        timestamps[name],
                        text=json.dumps({"camera": name}),
                    )
                if info["manifest"]["scene_objects"]:
                    record(
                        "nano_scene__objects",
                        timestamps["objects"],
                        text=json.dumps(unpack(pending["objects"])),
                    )
                measured.append(unpack(pending["frame_done"]))
                frame_count += 1
                if expected["frame"] == config["frames"] - 1:
                    result = outcome(expected["episode"], measured, model)
                    outcomes.append(result)
                    print(json.dumps(result), flush=True)
                    flush()
            send(node, "ack", [1.0], expected, "recorder")
            if expected["op"] == "finish":
                return
            request_id += 1
            barrier, pending = None, {}
    finally:
        flush()
        audit.close()


def main():
    config = json.loads(Path(os.environ["NANO_DORA_CONFIG"]).read_text())
    model = load_model(Path(config["model"]))
    role = sys.argv[1]
    node = Node()
    print(
        json.dumps({"node": role, "pid": os.getpid(), "status": "started"}), flush=True
    )
    try:
        {
            "task": task,
            "trajectory": trajectory,
            "limits": limits,
            "sim": sim,
            "recorder": recorder,
        }[role](node, config, model)
    except Exception as error:
        write_json(
            Path(config["output"]) / f"failure-{role}.json",
            dict(node=role, pid=os.getpid(), error=str(error), complete=False),
        )
        raise
    print(json.dumps({"node": role, "status": "complete"}), flush=True)


if __name__ == "__main__":
    main()
