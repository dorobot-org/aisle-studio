"""Fixed-base Nano simulation and AISLE -> Studio software-validation probe.

Uses AISLE's Nexus scene wrapper, Arrow trace schema and offline exporter.
This is an external integration probe, not a registered AISLE benchmark or a
hardware driver. Commands are joint-space motion checks, not manipulation tasks.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import math
import sys
import xml.etree.ElementTree as ET
from collections import defaultdict
from pathlib import Path


def load_model(path):
    root = ET.parse(path).getroot()
    joints = [j for j in root.findall("joint") if j.get("type") != "fixed"]
    names = [j.get("name") for j in joints]
    if len(names) != 17 or len(set(names)) != 17:
        raise ValueError("Expected the 17-channel Adora Nano model")
    groups = {
        side: [n for n in names if n.startswith(side + "_j")]
        for side in ("left", "right")
    }
    if any(len(g) != 6 for g in groups.values()):
        raise ValueError("Expected five joints and one gripper per arm")
    limits, velocities = {}, {}
    for j in joints:
        lim = j.find("limit")
        velocities[j.get("name")] = (
            float(lim.get("velocity", "inf")) if lim is not None else math.inf
        )
        limits[j.get("name")] = (
            (-math.inf, math.inf)
            if j.get("type") == "continuous"
            else (float(lim.get("lower")), float(lim.get("upper")))
        )
    return dict(
        names=names,
        limits=limits,
        velocities=velocities,
        held=[n for n in names if n not in groups["left"] + groups["right"]],
        **groups,
    )


def native_indices(wire_names, engine_names):
    if len(set(engine_names)) != len(engine_names) or set(wire_names) != set(
        engine_names
    ):
        raise ValueError("Simulator joints do not match the Nano model")
    return [engine_names.index(n) for n in wire_names]


def motion_target(model, mode, phase):
    if (
        mode not in ("left", "right", "both")
        or not math.isfinite(phase)
        or not 0 <= phase <= 1
    ):
        raise ValueError("Invalid motion or phase")
    active = model["left"] + model["right"] if mode == "both" else model[mode]
    pulse = math.sin(math.pi * phase) ** 2
    target = []
    for name in model["names"]:
        if name not in active:
            q = 0.0
        else:
            joint = int(name.split("_j")[1][0])
            amplitude = 0.25 if joint == 6 else 0.06 + 0.012 * joint
            direction = -1 if name.startswith("right_") and joint in (1, 5) else 1
            q = direction * amplitude * pulse
        low, high = model["limits"][name]
        if not low <= q <= high:
            raise ValueError(f"Target outside model limits: {name}")
        target.append(q)
    return target


def write_json(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, indent=2, sort_keys=True, allow_nan=False) + "\n")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def collect(args, model):
    import numpy as np
    import pyarrow as pa
    from aisle.harness.trace_recorder import TRACE_SCHEMA
    from aisle.sim.nexus_backend import _ensure_nexus, _new_scene

    source = args.output / "aisle-run"
    frames_dir = source / "traces/frames/overhead"
    frames_dir.mkdir(parents=True)
    engine = _ensure_nexus(args.backend)
    scene = _new_scene(
        engine, {"sim": {"dt": 0.01, "gravity": [0, 0, -9.81]}}, 1, [0.5] * 3
    )
    robot = scene.add_urdf_robot(args.model, "adora_nano", True)
    scene.add_box("floor", [2, 2, 0.1], [0, 0, -0.0843], fixed=True)
    objects, object_bodies = [], []
    if args.workspace:
        from nano_workspace import add_workspace

        objects, object_bodies = add_workspace(scene)
    rig, camera_specs = {}, {}
    if args.camera_rig == "nano":
        from nano_cameras import create_rig

        rig, camera_specs = create_rig(scene, robot)
    else:
        camera = scene.add_camera(
            (320, 240), 55, pos=(1.2, -1.2, 1.0), lookat=(0, 0, 0.35)
        )
        camera_specs["front"] = dict(
            video_key="observation.images.front",
            source_directory="overhead",
            resolution=[320, 240],
            fps=10,
            channels=["rgb"],
            optical_calibration="external observer, not a Nano sensor",
        )
    scene.build()
    by_offset = sorted((j.dofs_idx_local[0], j.name) for j in robot.joints if j.n_dofs)
    if any(j.n_dofs != 1 for j in robot.joints if j.n_dofs):
        raise ValueError("Expected one DOF per movable Nano joint")
    indices = native_indices(model["names"], [n for _, n in by_offset])
    grippers = [
        model["names"].index(side + "_j6_gripper") for side in ("left", "right")
    ]
    native_grippers = [indices[i] for i in grippers]
    rows = defaultdict(list)
    tick = 0

    def stamp():
        return tick * 10_000_000

    def record(topic, data=None, text=None):
        rows[topic].append(
            dict(
                sim_time_ns=stamp(),
                env_id=0,
                seq=len(rows[topic]),
                data=data,
                text=text,
            )
        )

    def step(count=1):
        nonlocal tick
        for _ in range(count):
            scene.step()
            tick += 1

    def state():
        q = np.asarray(robot.get_qpos(), dtype=np.float32)[indices]
        if q.shape != (17,) or not np.isfinite(q).all():
            raise ValueError("Simulator produced invalid joint state")
        return q

    def command(wire):
        native = np.empty(17, dtype=np.float32)
        native[indices] = wire
        robot.control_dofs_position(native)

    outcomes = []
    for episode, mode in enumerate(("left", "right", "both")):
        robot.set_qpos(np.zeros(17, dtype=np.float32))
        command(np.zeros(17, dtype=np.float32))
        record("nano_reset__reset_done", [1.0])
        for spec, body in zip(objects, object_bodies):
            body.set_pos(spec["position"])
            body.set_quat([1, 0, 0, 0])
            body.zero_all_dofs_velocity()
        peak_error, limit_excess = 0.0, 0.0
        measured, commanded = [], []
        for frame in range(args.frames):
            if args.workspace:
                from nano_workspace import workspace_target

                target = workspace_target(model, mode, frame / (args.frames - 1))
            else:
                target = motion_target(model, mode, frame / (args.frames - 1))
            step()
            if frame == 0:
                record(
                    "nano_task__target_request",
                    text=json.dumps(
                        {
                            "instruction": f"Software validation: {mode} arm joint motion; fixed base; simulation only"
                        }
                    ),
                )
            # Gripper and observation precede the arm command in simulation
            # time, so AISLE's strictly-earlier join needs no invented ordering.
            robot.control_dofs_position(
                [target[i] for i in grippers], dofs_idx_local=native_grippers
            )
            record("nano_limits__gripper_cmd_safe", [target[i] for i in grippers])
            step()
            q = state()
            if objects:
                from nano_workspace import capture_objects

                record(
                    "nano_scene__objects",
                    text=json.dumps(capture_objects(objects, object_bodies, stamp())),
                )
            if rig:
                from nano_cameras import capture_rig

                capture_rig(rig, camera_specs, frames_dir.parent, stamp())
                for name in rig:
                    record(
                        f"nano_camera__{name}",
                        text=json.dumps(
                            dict(
                                camera=name,
                                sim_time_ns=stamp(),
                                world_from_camera_gl=rig[name].transform.tolist(),
                            )
                        ),
                    )
            else:
                rgb = np.asarray(camera.render()[0], dtype=np.uint8)
                if rgb.shape != (240, 320, 3):
                    raise ValueError(f"Unexpected rendered image shape: {rgb.shape}")
                np.savez_compressed(frames_dir / f"{stamp():020d}.npz", rgb=rgb)
            record("nano_camera__rgb_overhead")
            record("nano_state__joint_state", q.tolist())
            step()
            command(target)
            record("nano_limits__joint_cmd_safe", target)
            step(7)
            after = state()
            measured.append(after.tolist())
            commanded.append(target)
            peak_error = max(peak_error, float(np.max(np.abs(after - target))))
            for name, value in zip(model["names"], after):
                low, high = model["limits"][name]
                limit_excess = max(
                    limit_excess, low - float(value), float(value) - high
                )
        active_sides = ("left", "right") if mode == "both" else (mode,)
        excursion = np.ptp(np.asarray(measured), axis=0)
        moving = {
            side: float(max(excursion[model["names"].index(n)] for n in model[side]))
            for side in ("left", "right")
        }
        ok = (
            peak_error < 0.08
            and limit_excess < 0.03
            and all(moving[s] > 0.03 for s in active_sides)
        )
        outcomes.append(
            dict(
                episode=episode,
                env_id=0,
                status="success" if ok else "fail",
                task_family="nano-studio-motion-check",
                mode=mode,
                peak_tracking_error_rad=peak_error,
                max_joint_limit_excess_rad=limit_excess,
                measured_arm_excursion_rad=moving,
                acceptance="finite state, tracking error <0.08 rad, limit excess <0.03 rad, each commanded arm excursion >0.03 rad; software fixture only",
            )
        )
        print(json.dumps(outcomes[-1]), file=sys.stderr, flush=True)
    robot.set_qpos(np.zeros(17, dtype=np.float32))
    command(np.zeros(17, dtype=np.float32))
    record("nano_reset__reset_done", [1.0])
    for name, values in rows.items():
        table = pa.Table.from_pylist(values, schema=TRACE_SCHEMA)
        with (
            pa.OSFile(str(source / "traces" / (name + ".arrow")), "wb") as sink,
            pa.ipc.new_stream(sink, TRACE_SCHEMA) as writer,
        ):
            writer.write_table(table)
    (source / "episodes.jsonl").write_text(
        "".join(json.dumps(v) + "\n" for v in outcomes)
    )
    write_json(
        source / "manifest.json",
        dict(
            embodiment="adora_nano",
            sim_engine="nexus",
            sim_backend=args.backend,
            evidence_kind="simulated_joint_motion_software_fixture",
            hardware_connected=False,
            fixed_base=True,
            head_and_wheels="zero position targets",
            state_joint_names=model["names"],
            units="radians",
            physics_dt_s=0.01,
            capture_fps=10,
            cameras=camera_specs,
            scene_objects=objects,
            motion="multi-stage free-space arm sweep"
            if args.workspace
            else "small joint pulse",
            segmentation_id_map={
                str(k): v for k, v in scene.segmentation_idx_dict.items()
            },
            segmentation_entities=[e.name for e in scene.entities] + [robot.name],
            segmentation_robot_links=[link.name for link in robot.links],
            camera_rig=args.camera_rig,
            capture_policy="every scheduled 10 Hz sensor frame, no frame subsampling",
            source_model=str(args.model),
            model_sha256=digest(args.model),
            nexus_version=importlib.metadata.version("dimforge-nexus3d"),
            aisle_modules={
                p: digest(args.aisle / p)
                for p in (
                    "src/aisle/sim/nexus_backend.py",
                    "src/aisle/sim/nexus_physics.toml",
                    "src/aisle/harness/trace_recorder.py",
                    "tools/demo_dataset.py",
                )
            },
            command_validation="URDF-limit checked probe targets; not a certified AISLE graph or benchmark",
            model_dynamics="nominal inertias; AISLE default URDF gains/damping; not hardware calibrated",
        ),
    )
    contract = dict(
        embodiment="adora_nano",
        state=dict(names=model["names"], units=["rad"] * 17, frame="joint"),
        forwarded=dict(
            names=model["names"],
            units=["rad"] * 17,
            frame="joint",
            semantics="position_target",
        ),
        gripper=dict(
            names=[model["names"][i] for i in grippers],
            units=["rad"] * 2,
            frame="joint",
            semantics="position_target",
        ),
        training_action="full_forwarded_position_targets_17",
    )
    write_json(source / "contract.json", contract)
    return source, contract, outcomes


def to_lerobot(export, destination, model, source_run):
    import cv2
    import numpy as np
    import pyarrow as pa
    import pyarrow.parquet as pq
    from demo_dataset import read_dataset

    data = read_dataset(export)
    source_manifest = json.loads((source_run / "manifest.json").read_text())
    cameras = source_manifest["cameras"]
    object_rows = {}
    if source_manifest.get("scene_objects"):
        with pa.memory_map(
            str(source_run / "traces/nano_scene__objects.arrow"), "r"
        ) as f:
            object_rows = {
                r["sim_time_ns"]: json.loads(r["text"])
                for r in pa.ipc.open_stream(f).read_all().to_pylist()
            }
    meta = destination / "meta"
    meta.mkdir(parents=True)
    episodes, tasks, mapping = [], [], []
    total = 0
    for index, episode in enumerate(data["episodes"]):
        samples = episode["samples"]
        if not samples or any(not s["eligible"] for s in samples):
            raise ValueError(
                "Refusing to hide ineligible AISLE samples in the Studio export"
            )
        width = len(model["names"])
        states = np.asarray([s["state"]["data"] for s in samples], dtype=np.float32)
        actions = np.asarray(
            [s["forwarded"]["data"] for s in samples], dtype=np.float32
        )
        first = samples[0]["forwarded"]["sim_time_ns"]
        times = [(s["forwarded"]["sim_time_ns"] - first) / 1e9 for s in samples]
        if not np.allclose(times, np.arange(len(samples)) / 10, rtol=0, atol=1e-9):
            raise ValueError(
                "This probe requires a regular 10 Hz recording; no silent resampling"
            )
        task = samples[0]["instruction"]["text"]
        table = pa.table(
            {
                "observation.state": pa.FixedSizeListArray.from_arrays(
                    pa.array(states.ravel(), type=pa.float32()), width
                ),
                "action": pa.FixedSizeListArray.from_arrays(
                    pa.array(actions.ravel(), type=pa.float32()), width
                ),
                "timestamp": pa.array(times, type=pa.float64()),
                "frame_index": pa.array(range(len(samples)), type=pa.int64()),
                "episode_index": pa.array([index] * len(samples), type=pa.int64()),
                "index": pa.array(range(total, total + len(samples)), type=pa.int64()),
                "task_index": pa.array([index] * len(samples), type=pa.int64()),
            }
        )
        parquet = destination / f"data/chunk-000/episode_{index:06d}.parquet"
        parquet.parent.mkdir(parents=True, exist_ok=True)
        pq.write_table(table, parquet, compression="snappy")
        episode_mapping = []
        for frame, sample in enumerate(samples):
            ts = sample["frame"]["sim_time_ns"]
            episode_mapping.append(
                dict(
                    episode=index,
                    frame=frame,
                    observation_sim_time_ns=sample["state"]["sim_time_ns"],
                    camera_sim_time_ns=ts,
                    command_sim_time_ns=sample["forwarded"]["sim_time_ns"],
                    source_frame_sha256=digest(
                        export / f"source/traces/frames/overhead/{ts:020d}.npz"
                    ),
                    source_command=sample["forwarded"]["source"],
                    source_command_row=sample["forwarded"]["row"],
                    cameras={},
                )
            )
        for name, spec in cameras.items():
            key = spec["video_key"]
            video_path = destination / f"videos/chunk-000/{key}/episode_{index:06d}.mp4"
            video_path.parent.mkdir(parents=True, exist_ok=True)
            video = cv2.VideoWriter(
                str(video_path),
                cv2.VideoWriter_fourcc(*"mp4v"),
                10,
                tuple(spec["resolution"]),
            )
            if not video.isOpened():
                raise ValueError(f"Video encoder unavailable for {name}")
            try:
                for row in episode_mapping:
                    ts = row["camera_sim_time_ns"]
                    relative = f"traces/frames/{spec['source_directory']}/{ts:020d}.npz"
                    source = source_run / relative
                    with np.load(source, allow_pickle=False) as raw:
                        if "sim_time_ns" in raw and int(raw["sim_time_ns"]) != ts:
                            raise ValueError(f"Camera timestamp mismatch: {name}")
                        video.write(cv2.cvtColor(raw["rgb"], cv2.COLOR_RGB2BGR))
                    row["cameras"][name] = dict(
                        sim_time_ns=ts, source=relative, sha256=digest(source)
                    )
            finally:
                video.release()
            decoder = cv2.VideoCapture(str(video_path))
            decoded = 0
            while decoder.read()[0]:
                decoded += 1
            decoder.release()
            if decoded != len(samples):
                raise ValueError(f"Video frame count disagrees with Parquet: {name}")
        mapping.extend(episode_mapping)
        if object_rows:
            write_json(
                meta / f"scene/episode_{index:06d}.json",
                dict(
                    version=1,
                    objects=source_manifest["scene_objects"],
                    frames=[
                        dict(timestamp=t, **object_rows[row["camera_sim_time_ns"]])
                        for t, row in zip(times, episode_mapping)
                    ],
                ),
            )
        episodes.append(dict(episode_index=index, tasks=[task], length=len(samples)))
        tasks.append(dict(task_index=index, task=task))
        total += len(samples)
    features = {
        k: dict(dtype="float32", shape=[17], names=model["names"])
        for k in ("observation.state", "action")
    }
    features.update(
        {
            k: dict(
                dtype="float64" if k == "timestamp" else "int64", shape=[1], names=None
            )
            for k in (
                "timestamp",
                "frame_index",
                "episode_index",
                "index",
                "task_index",
            )
        }
    )
    for spec in cameras.values():
        width, height = spec["resolution"]
        features[spec["video_key"]] = dict(
            dtype="video",
            shape=[height, width, 3],
            names=["height", "width", "channel"],
            info={
                "video.fps": 10,
                "video.codec": "mpeg4",
                "video.pix_fmt": "yuv420p",
                "video.is_depth_map": False,
                "has_audio": False,
            },
        )
    write_json(
        meta / "info.json",
        dict(
            codebase_version="v2.1",
            robot_type="adora_nano",
            fps=10,
            total_episodes=len(episodes),
            total_frames=total,
            total_tasks=len(tasks),
            total_videos=len(episodes) * len(cameras),
            total_chunks=1,
            chunks_size=1000,
            splits={"train": f"0:{len(episodes)}"},
            data_path="data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
            video_path="videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4",
            features=features,
        ),
    )
    for filename, values in (("episodes.jsonl", episodes), ("tasks.jsonl", tasks)):
        (meta / filename).write_text("".join(json.dumps(v) + "\n" for v in values))
    write_json(
        meta / "aisle_provenance.json",
        dict(
            kind="physics_simulation_software_validation_fixture",
            hardware_connected=False,
            units="radians",
            joint_order=model["names"],
            source_manifest_sha256=digest(export / "manifest.json"),
            outcomes=[e["outcome"] for e in data["episodes"]],
            mapping=mapping,
            cameras=cameras,
            raw_recording="../aisle-run (retain alongside this dataset for lossless RGB, depth, segmentation and camera poses)",
            raw_recording_sha256={
                str(p.relative_to(source_run)): digest(p)
                for p in sorted(source_run.rglob("*"))
                if p.is_file()
            },
            aisle_export_scope="AISLE joins head/overhead RGB; wrist streams retained in raw recording and added to Studio with exact same-tick joins",
            limitations=[
                "not a manipulation benchmark",
                "nominal optical calibration; not measured hardware intrinsics",
                "fixed base",
                "nominal model dynamics",
                "no hardware actuation",
            ],
        ),
    )
    return dict(
        episodes=len(episodes), frames=total, channels=17, cameras=len(cameras), fps=10
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--aisle",
        type=Path,
        default=Path(__file__).resolve().parents[2].parent / "AISLE",
    )
    parser.add_argument(
        "--model",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf",
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--backend", choices=["metal", "webgpu", "cpu"], default="metal"
    )
    parser.add_argument("--frames", type=int, default=30)
    parser.add_argument("--transport", choices=["dora", "direct"], default="dora")
    parser.add_argument("--dora", help="Dora 1.0.1 executable (default: PATH)")
    parser.add_argument(
        "--timeout", type=int, default=600, help="Maximum Dora run wall time in seconds"
    )
    parser.add_argument("--camera-rig", choices=["observer", "nano"], default="nano")
    parser.add_argument(
        "--workspace",
        action="store_true",
        help="Six free cubes and a visible multi-stage arm sweep; not a grasp-success task",
    )
    args = parser.parse_args()
    if args.frames < 3 or args.output.exists():
        parser.error("Use at least three frames and a new output directory")
    args.aisle, args.model, args.output = (
        p.resolve() for p in (args.aisle, args.model, args.output)
    )
    sys.path.insert(0, str(args.aisle / "tools"))
    from demo_dataset import export_dataset

    model = load_model(args.model)
    if args.transport == "dora":
        from nano_dora import collect_dora

        source, contract, outcomes = collect_dora(args, model)
    else:
        source, contract, outcomes = collect(args, model)
    export = args.output / "aisle-export"
    exported = export_dataset(source, export, contract=contract)
    dataset = to_lerobot(export, args.output / "lerobot", model, source)
    result = dict(
        ok=all(o["status"] == "success" for o in outcomes),
        scope="external Nano/AISLE/Nexus/Studio integration probe",
        transport=args.transport,
        output=str(args.output),
        dataset=dataset,
        aisle_export_coverage=exported["coverage"],
        outcomes=outcomes,
    )
    write_json(args.output / "result.json", result)
    print(json.dumps(result, allow_nan=False))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
