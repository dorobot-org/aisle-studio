"""Dora actors for closed-loop Nano navigation, complete recording and live UI."""

from __future__ import annotations
import io
import json
import os
import sys
import time
import traceback
from pathlib import Path
import numpy as np
import pyarrow as pa
from dora import Node
from nano_living import (
    CAMERAS,
    CONTROL_DT,
    NAVIGATION_POSTURE,
    LivingSimulation,
    Navigator,
    WheelGuard,
)


def send(node, name, value, meta):
    if isinstance(value, dict):
        value = pa.array([json.dumps(value, allow_nan=False)])
    elif isinstance(value, bytes):
        value = pa.array(np.frombuffer(value, dtype=np.uint8))
    elif not isinstance(value, pa.Array):
        value = pa.array(np.asarray(value, dtype=np.float64))
    node.send_output(name, value, {**meta, "sender_pid": os.getpid()})


def clean_meta(event):
    return {k: v for k, v in event["metadata"].items() if k != "timestamp"}


def unpack_info(event):
    return json.loads(event["value"][0].as_py())


def arrays(event):
    raw = event["value"].to_numpy(zero_copy_only=False).tobytes()
    with np.load(io.BytesIO(raw), allow_pickle=False) as data:
        return {k: data[k] for k in data.files}


def events(node):
    while True:
        event = node.next(timeout=120)
        if event is None or event["type"] in ("ERROR", "STOP"):
            raise RuntimeError(f"Dora stopped unexpectedly: {event}")
        if event["type"] == "INPUT":
            yield event


def wait_recorded(node, sequence):
    """Keep final producers alive until their last source frame is recorded."""
    for event in events(node):
        if event["id"] == "ack" and int(clean_meta(event)["sequence"]) == sequence:
            return


def sim(node, config):
    import cv2
    from nano_cameras import read_rig

    simulation = LivingSimulation(
        Path(config["model"]),
        config["seed"],
        resolution=(config["width"], config["height"]),
    )
    names = simulation.names
    robot_bodies = {
        simulation.scene.gpu_index(0, b) for b in simulation.robot.link_bodies[0]
    }
    furniture = {
        simulation.scene.gpu_index(0, b.handles[0])
        for b, s in zip(simulation.bodies, simulation.specs)
        if s["fixed"] and s["name"] != "floor"
    }
    cube_index = next(
        i for i, s in enumerate(simulation.specs) if s["name"] == "cube_6"
    )
    cube_body = simulation.scene.gpu_index(0, simulation.bodies[cube_index].handles[0])
    info = dict(
        version=1,
        session="Nano living room " + config["run_id"],
        joint_names=names,
        objects=[
            {k: s[k] for k in ("name", "size", "color")} for s in simulation.specs
        ],
        specs=simulation.specs,
        cameras=simulation.camera_specs,
        fps=1 / CONTROL_DT,
        physics_dt=0.01,
        contact_audit_hz=100,
        preview_encoding="JPEG quality 90; lossless sources go only to recorder",
        robot_body_ids=sorted(robot_bodies),
        furniture_body_ids=sorted(furniture),
        push_cube_body_id=cube_body,
        segmentation_id_map={
            str(k): v for k, v in simulation.scene.segmentation_idx_dict.items()
        },
        segmentation_entities=[e.name for e in simulation.scene.entities]
        + [simulation.robot.name],
        segmentation_robot_links=[link.name for link in simulation.robot.links],
        model=config["model"],
        robot_visuals=simulation.visuals,
        navigation_posture=NAVIGATION_POSTURE,
        recording=not config.get("preview_only", False),
        continuous=config.get("continuous", False),
        initial_objects=simulation.cube_start,
    )
    send(node, "scene_info", info, {})
    ready = not config["wait_viewer"]
    started = False
    last_seq = -1
    clock = time.monotonic()

    def observe(seq):
        send(
            node,
            "observation",
            simulation.observation(),
            {"sequence": seq, "sim_time_ns": simulation.tick * 10_000_000},
        )

    if ready:
        observe(0)
        started = True
    guard = WheelGuard()
    for event in events(node):
        if event["id"] == "ack":
            continue
        if event["id"] == "viewer_ready":
            if not started:
                observe(0)
                started = True
                clock = time.monotonic()
            continue
        meta = clean_meta(event)
        seq = int(meta["sequence"])
        if seq != last_seq + 1 or "guard_pid" not in meta:
            raise ValueError("Unordered or unguarded physics command")
        rates = event["value"].to_numpy(zero_copy_only=False)
        # The simulator independently validates ordering, finite values and limits.
        checked = guard.accept(rates, seq)
        if not np.allclose(checked, rates):
            raise ValueError("Guard acceleration contract disagrees")
        profile_start = time.perf_counter()
        contacts = simulation.advance(rates, capture_contacts=True)
        obs = simulation.observation()
        snap = simulation.snapshot()
        physics_ms = (time.perf_counter() - profile_start) * 1000
        bad = []
        pushing = False
        for _, a, b in contacts:
            a, b = int(a), int(b)
            if (a in robot_bodies and b in furniture) or (
                b in robot_bodies and a in furniture
            ):
                bad.append([a, b])
            if (a in robot_bodies and b == cube_body) or (
                b in robot_bodies and a == cube_body
            ):
                pushing = True
        state = dict(
            observation=obs,
            transforms=np.asarray(snap["transforms"], np.float32),
            contacts=np.asarray(bad, dtype=np.int64).reshape(-1, 2),
            cube_contact=np.array([pushing], np.uint8),
            physics_contacts=contacts,
            physics_ticks=np.arange(
                simulation.tick - round(CONTROL_DT / 0.01) + 1,
                simulation.tick + 1,
                dtype=np.int64,
            ),
        )
        camera_start = time.perf_counter()
        for name, camera in read_rig(
            simulation.rig, simulation.camera_specs, snap["sim_time_ns"]
        ):
            for key, value in camera.items():
                state[f"{name}__{key}"] = value
        camera_ms = (time.perf_counter() - camera_start) * 1000
        pack_start = time.perf_counter()
        buffer = io.BytesIO()
        # Continuous viewing keeps only a small state/ack packet. Camera
        # sources are saved only by explicitly selected finite recording runs.
        record_data = (
            {k: v for k, v in state.items() if "__" not in k}
            if config.get("preview_only")
            else state
        )
        np.savez(buffer, **record_data)
        pack_ms = (time.perf_counter() - pack_start) * 1000
        meta.update(
            sim_time_ns=snap["sim_time_ns"],
            sim_pid=os.getpid(),
            capture_wall_ns=time.time_ns(),
            physics_ms=physics_ms,
            camera_ms=camera_ms,
            pack_ms=pack_ms,
        )
        send(node, "frame", buffer.getvalue(), meta)
        # A stopped preview must not queue the full RGB/depth/segmentation
        # payload inside Dora's NextEvent RPC (64 MiB maximum). Keep its
        # independent stream small; the recorder still receives all sources.
        preview_data = dict(observation=obs, transforms=state["transforms"])
        for name in CAMERAS:
            ok, jpeg = cv2.imencode(
                ".jpg",
                cv2.cvtColor(state[f"{name}__rgb"], cv2.COLOR_RGB2BGR),
                [cv2.IMWRITE_JPEG_QUALITY, 90],
            )
            if not ok:
                raise RuntimeError(f"Could not encode preview camera {name}")
            preview_data[f"{name}__jpeg"] = jpeg
        preview_buffer = io.BytesIO()
        np.savez(preview_buffer, **preview_data)
        send(node, "preview_frame", preview_buffer.getvalue(), meta)
        last_seq = seq
        if bool(meta.get("finished", False)):
            wait_recorded(node, seq)
            return
        # Wall pacing never invents simulation time; slow machines report RTF < 1.
        if config["realtime"]:
            time.sleep(max(0.0, clock + (seq + 1) * CONTROL_DT - time.monotonic()))
        observe(seq + 1)


def nav(node, config):
    navigator = None
    pending = None
    ack = -1
    done_ticks = 0
    for event in events(node):
        if event["id"] == "scene_info":
            navigator = Navigator(
                unpack_info(event)["specs"],
                config.get("goals"),
                config.get("continuous", False),
            )
            continue
        if event["id"] == "ack":
            ack = int(clean_meta(event)["sequence"])
        elif event["id"] == "observation":
            pending = event
        if pending is None or navigator is None:
            continue
        meta = clean_meta(pending)
        seq = int(meta["sequence"])
        # At most two unacknowledged frames: overlap recording frame n with
        # physics n+1, while bounded Dora queues still preserve every source.
        if seq > ack + 2:
            continue
        obs = pending["value"].to_numpy(zero_copy_only=False)
        rates = navigator.command(obs[:3], obs[3:7])
        if navigator.completed:
            done_ticks += 1
        finished = done_ticks >= round(1 / CONTROL_DT) or (
            config["max_frames"] > 0 and seq >= config["max_frames"] - 1
        )
        phase = (
            "Braking"
            if navigator.completed
            else (
                "Push cube"
                if navigator.goal_index == 0
                else f"Navigate {navigator.goal_index}/{len(navigator.goals) - 1}"
            )
        )
        if config.get("preview_only"):
            phase = f"Preview only · leg {navigator.legs + 1} · {phase}"
        meta.update(
            nav_pid=os.getpid(),
            goal_index=navigator.goal_index,
            phase=phase,
            finished=finished,
            success=navigator.completed,
            completed_legs=navigator.legs,
        )
        send(node, "wheel_cmd", rates, meta)
        pending = None
        if finished:
            (Path(config["output"]) / "navigation-plan.json").write_text(
                json.dumps(navigator.plans, indent=2)
            )
            wait_recorded(node, seq)
            return


def guard(node, config):
    gate = WheelGuard()
    for event in events(node):
        if event["id"] != "command":
            continue
        meta = clean_meta(event)
        rates = gate.accept(
            event["value"].to_numpy(zero_copy_only=False), int(meta["sequence"])
        )
        send(node, "safe_wheel_cmd", rates, {**meta, "guard_pid": os.getpid()})
        if bool(meta.get("finished", False)):
            wait_recorded(node, int(meta["sequence"]))
            return


def recorder(node, config):
    from nano_living_record import LivingRecorder, PreviewProgress

    recording = None
    pending = {}
    proposed = {}
    commands = {}
    next_seq = 0
    for event in events(node):
        topic = event["id"]
        meta = clean_meta(event)
        if topic == "scene_info":
            writer = PreviewProgress if config.get("preview_only") else LivingRecorder
            recording = writer(config, unpack_info(event))
            continue
        seq = int(meta["sequence"])
        if topic == "proposed":
            proposed[seq] = event["value"].to_pylist()
        elif topic == "command":
            commands[seq] = event["value"].to_pylist()
        else:
            pending[seq] = (arrays(event), meta)
        if (
            recording is None
            or next_seq not in pending
            or next_seq not in commands
            or next_seq not in proposed
        ):
            continue
        data, meta = pending.pop(next_seq)
        recording.append(data, meta, proposed.pop(next_seq), commands.pop(next_seq))
        send(
            node,
            "ack",
            [next_seq],
            {"sequence": next_seq, "sim_time_ns": meta["sim_time_ns"]},
        )
        next_seq += 1
        if bool(meta.get("finished", False)):
            result = recording.finish(bool(meta.get("success", False)))
            send(node, "recording_result", result, {})
            return


def preview(node, config):
    from nano_living_stream import PreviewHub

    hub = PreviewHub(config["socket"])
    info = None
    ready = False
    last = None
    try:
        while True:
            if not ready and (hub.clients > 0 or not config["wait_viewer"]):
                send(node, "viewer_ready", [1.0], {})
                ready = True
            event = node.next(timeout=0.1)
            if event is None:
                continue
            if event["type"] == "ERROR" and "timeout" in str(event).lower():
                continue
            if event["type"] in ("ERROR", "STOP"):
                raise RuntimeError(f"Preview stopped before recording result: {event}")
            if event["type"] != "INPUT":
                continue
            if event["id"] == "scene_info":
                info = unpack_info(event)
                continue
            if event["id"] == "recording_result":
                result = unpack_info(event)
                if last:
                    header, images = last
                    header = dict(
                        header,
                        finished=True,
                        sequence=header["sequence"] + 1,
                        dataset=result["dataset"],
                        phase="Complete" if result["success"] else "Stopped",
                    )
                    hub.publish(header, images)
                time.sleep(3.0)
                return
            if info is None:
                raise ValueError("Preview frame before scene metadata")
            data = arrays(event)
            meta = clean_meta(event)
            obs = data["observation"]
            images = []
            cameras = []
            for name in CAMERAS:
                import cv2

                bgr = cv2.imdecode(data[f"{name}__jpeg"], cv2.IMREAD_COLOR)
                if bgr is None:
                    raise ValueError(f"Invalid preview image {name}")
                h, w, _ = bgr.shape
                pixels = cv2.cvtColor(bgr, cv2.COLOR_BGR2BGRA).tobytes()
                images.append(pixels)
                cameras.append(dict(name=name, width=w, height=h, bytes=len(pixels)))
            header = dict(
                version=1,
                session=info["session"],
                sequence=int(meta["sequence"]),
                sim_time_ns=int(meta["sim_time_ns"]),
                fps=info["fps"],
                joint_names=info["joint_names"],
                joints=obs[7:24].tolist(),
                targets=obs[24:41].tolist(),
                base_position=obs[:3].tolist(),
                base_quat_wxyz=obs[3:7].tolist(),
                objects=info["objects"],
                transforms=data["transforms"].tolist(),
                cameras=cameras,
                phase=meta["phase"],
                finished=False,
            )
            last = (header, images)
            hub.publish(header, images)
    finally:
        hub.close()


def main():
    config = json.loads(Path(os.environ["NANO_LIVING_CONFIG"]).read_text())
    role = sys.argv[1]
    (Path(config["output"]) / f"node-{role}.json").write_text(
        json.dumps({"role": role, "pid": os.getpid()})
    )
    try:
        globals()[role](Node(), config)
    except BaseException as error:
        (Path(config["output"]) / f"failure-{role}.json").write_text(
            json.dumps(
                {"role": role, "error": str(error), "traceback": traceback.format_exc()}
            )
        )
        raise


if __name__ == "__main__":
    main()
