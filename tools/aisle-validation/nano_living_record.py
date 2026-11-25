"""Lossless synchronized sources plus a complete LeRobot/Studio export."""

from __future__ import annotations
import hashlib
import json
import os
import time
import zipfile
from pathlib import Path
import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
from nano_living import CAMERAS


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data, indent=2, allow_nan=False))
    temporary.replace(path)


def save_source(path, **arrays):
    """Standard lossless NPZ at level 1; level 6 stalls the live control barrier."""
    with zipfile.ZipFile(
        path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=1
    ) as archive:
        for name, value in arrays.items():
            with archive.open(name + ".npy", "w", force_zip64=True) as member:
                np.lib.format.write_array(member, np.asarray(value), allow_pickle=False)


class PreviewProgress:
    """Bounded live telemetry and acknowledgments; no camera archive or videos."""

    def __init__(self, config, info):
        self.root = Path(config["output"])
        self.info = info
        self.started = time.monotonic()
        self.expected = 0
        self.bad_contacts = 0
        write_json(self.root / "scene-info.json", info)

    def append(self, data, meta, proposed, command):
        seq = int(meta["sequence"])
        stamp = int(meta["sim_time_ns"])
        if seq != self.expected or stamp != round((seq + 1) / self.info["fps"] * 1e9):
            raise ValueError("Missing or mistimed live state")
        obs = data["observation"]
        if obs.shape != (41,) or not np.isfinite(obs).all():
            raise ValueError("Invalid live robot state")
        self.expected += 1
        self.bad_contacts += bool(len(data["contacts"]))
        write_json(
            self.root / "progress.json",
            dict(
                frame=seq,
                sim_time=stamp * 1e-9,
                phase=meta["phase"],
                completed_legs=int(meta.get("completed_legs", 0)),
                base=obs[:7].tolist(),
                joints=obs[7:24].tolist(),
                wall_seconds=time.monotonic() - self.started,
                preview_frames=self.expected,
                raw_frames=0,
                recording=False,
                furniture_contact_frames=self.bad_contacts,
            ),
        )

    def finish(self, navigation_success):
        result = dict(
            success=navigation_success,
            navigation_success=navigation_success,
            preview_only=True,
            preview_frames=self.expected,
            recording=False,
            dataset=None,
            raw=None,
            sim_seconds=self.expected / self.info["fps"],
            wall_seconds=time.monotonic() - self.started,
        )
        write_json(self.root / "result.json", result)
        return result


class LivingRecorder:
    def __init__(self, config, info):
        import cv2

        self.cv2 = cv2
        self.config = config
        self.info = info
        self.root = Path(config["output"])
        self.dataset = self.root / "lerobot"
        self.source = self.root / "raw"
        self.source.mkdir()
        self.rows = []
        self.scene_frames = []
        self.writers = {}
        self.started = time.monotonic()
        self.contact_frames = 0
        self.bad_contacts = []
        self.pids = set()
        self.expected = 0
        self.audit = (self.source / "frames.jsonl").open("w")
        for name in CAMERAS:
            path = (
                self.dataset
                / f"videos/chunk-000/observation.images.{name}/episode_000000.mp4"
            )
            path.parent.mkdir(parents=True, exist_ok=True)
            writer = cv2.VideoWriter(
                str(path),
                cv2.VideoWriter_fourcc(*"mp4v"),
                info["fps"],
                (config["width"], config["height"]),
            )
            if not writer.isOpened():
                raise RuntimeError(f"Cannot open video encoder: {name}")
            self.writers[name] = writer
        write_json(self.root / "scene-info.json", info)

    def append(self, data, meta, proposed, command):
        append_start = time.perf_counter()
        seq = int(meta["sequence"])
        stamp = int(meta["sim_time_ns"])
        fps = self.info["fps"]
        if seq != self.expected or stamp != round((seq + 1) / fps * 1e9):
            raise ValueError("Missing, reordered or mistimed recording frame")
        obs = data["observation"]
        transforms = data["transforms"]
        if (
            obs.shape != (41,)
            or transforms.shape != (len(self.info["objects"]), 16)
            or not np.isfinite(obs).all()
            or not np.isfinite(transforms).all()
        ):
            raise ValueError("Invalid state/scene recording")
        for name in CAMERAS:
            rgb = data[f"{name}__rgb"]
            depth = data[f"{name}__depth"]
            seg = data[f"{name}__seg"]
            if (
                rgb.shape != (self.config["height"], self.config["width"], 3)
                or depth.shape != rgb.shape[:2]
                or seg.shape != depth.shape
                or int(data[f"{name}__sim_time_ns"]) != stamp
            ):
                raise ValueError("Camera timing/shape mismatch")
            if not np.isfinite(depth).all():
                raise ValueError("Invalid depth")
            self.writers[name].write(self.cv2.cvtColor(rgb, self.cv2.COLOR_RGB2BGR))
        path = self.source / f"frame_{seq:06d}.npz"
        save_source(
            path,
            **data,
            command=np.asarray(command),
            proposed=np.asarray(proposed),
            sim_time_ns=np.int64(stamp),
        )
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        pids = {k: int(v) for k, v in meta.items() if k.endswith("_pid")}
        self.pids.update(pids.values())
        self.pids.add(os.getpid())
        self.audit.write(
            json.dumps(
                dict(
                    frame=seq,
                    sim_time_ns=stamp,
                    sha256=digest,
                    pids=pids,
                    phase=meta["phase"],
                    goal_index=int(meta["goal_index"]),
                )
            )
            + "\n"
        )
        self.audit.flush()
        self.rows.append(
            dict(
                state=obs[7:24].astype(np.float32),
                action=obs[24:41].astype(np.float32),
                base=obs[:7].tolist(),
                timestamp=seq / fps,
                command=command,
                proposed=proposed,
                sim_time_ns=stamp,
            )
        )
        self.scene_frames.append(
            dict(
                timestamp=seq / fps,
                sim_time_ns=stamp,
                transforms=transforms.tolist(),
                base_pose=dict(
                    position=obs[:3].tolist(), quat_xyzw=obs[[4, 5, 6, 3]].tolist()
                ),
            )
        )
        self.contact_frames += int(bool(data["cube_contact"][0]))
        if len(data["contacts"]):
            self.bad_contacts.append(dict(frame=seq, pairs=data["contacts"].tolist()))
        self.expected += 1
        write_json(
            self.root / "progress.json",
            dict(
                frame=seq,
                sim_time=stamp * 1e-9,
                phase=meta["phase"],
                base=obs[:7].tolist(),
                wall_seconds=time.monotonic() - self.started,
                raw_frames=self.expected,
                processing_ms=(time.perf_counter() - append_start) * 1000,
                physics_ms=meta.get("physics_ms"),
                camera_ms=meta.get("camera_ms"),
                pack_ms=meta.get("pack_ms"),
            ),
        )

    def finish(self, navigation_success):
        for writer in self.writers.values():
            writer.release()
        self.audit.close()
        n = len(self.rows)
        fps = self.info["fps"]
        meta = self.dataset / "meta"
        meta.mkdir(parents=True, exist_ok=True)
        states = np.array([r["state"] for r in self.rows], np.float32)
        actions = np.array([r["action"] for r in self.rows], np.float32)
        task = self.config.get("task", "Navigate the living room around a sofa and coffee table; push a floor cube with physical contact")
        table = pa.table(
            {
                "observation.state": pa.FixedSizeListArray.from_arrays(
                    pa.array(states.ravel()), 17
                ),
                "action": pa.FixedSizeListArray.from_arrays(
                    pa.array(actions.ravel()), 17
                ),
                "timestamp": pa.array([r["timestamp"] for r in self.rows]),
                "frame_index": pa.array(range(n), type=pa.int64()),
                "episode_index": pa.array([0] * n, type=pa.int64()),
                "index": pa.array(range(n), type=pa.int64()),
                "task_index": pa.array([0] * n, type=pa.int64()),
            }
        )
        path = self.dataset / "data/chunk-000/episode_000000.parquet"
        path.parent.mkdir(parents=True, exist_ok=True)
        pq.write_table(table, path, compression="snappy")
        features = {
            name: dict(dtype="float32", shape=[17], names=self.info["joint_names"])
            for name in ["observation.state", "action"]
        }
        for name in [
            "timestamp",
            "frame_index",
            "episode_index",
            "index",
            "task_index",
        ]:
            features[name] = dict(
                dtype="float32" if name == "timestamp" else "int64",
                shape=[1],
                names=None,
            )
        for name in CAMERAS:
            features[f"observation.images.{name}"] = dict(
                dtype="video",
                shape=[self.config["height"], self.config["width"], 3],
                names=["height", "width", "channels"],
                info={
                    "video.fps": fps,
                    "video.codec": "mpeg4",
                    "video.pix_fmt": "yuv420p",
                    "video.is_depth_map": False,
                    "has_audio": False,
                },
            )
        write_json(
            meta / "scene/episode_000000.json",
            dict(version=2, objects=self.info["objects"], frames=self.scene_frames),
        )
        (meta / "episodes.jsonl").write_text(
            json.dumps(dict(episode_index=0, tasks=[task], length=n)) + "\n"
        )
        (meta / "tasks.jsonl").write_text(
            json.dumps(dict(task_index=0, task=task)) + "\n"
        )
        # Commit info.json last so Studio never opens a partial recording.
        write_json(
            meta / "info.json",
            dict(
                codebase_version="v2.1",
                robot_type="adora_nano",
                fps=fps,
                total_episodes=1,
                total_frames=n,
                total_tasks=1,
                total_videos=3,
                total_chunks=1,
                chunks_size=1000,
                splits={"train": "0:1"},
                data_path="data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
                video_path="videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4",
                features=features,
            ),
        )
        base = np.array([r["base"][:3] for r in self.rows])
        distance = float(np.linalg.norm(np.diff(base[:, :2], axis=0), axis=1).sum())
        cube_index = next(
            i for i, o in enumerate(self.info["objects"]) if o["name"] == "cube_6"
        )
        start = np.array(self.info["initial_objects"][cube_index])
        end = np.array(self.scene_frames[-1]["transforms"][cube_index])[12:15]
        displacement = float(np.linalg.norm(end[:2] - start[:2]))
        success = (
            navigation_success
            and not self.bad_contacts
            and self.contact_frames > 0
            and displacement > 0.15
            and distance > 3
        )
        nodes = {
            p.stem.removeprefix("node-"): json.loads(p.read_text())["pid"]
            for p in self.root.glob("node-*.json")
        }
        result = dict(
            success=success,
            navigation_success=navigation_success,
            frames=n,
            camera_frames={name: n for name in CAMERAS},
            fps=fps,
            sim_seconds=n / fps,
            wall_seconds=time.monotonic() - self.started,
            base_distance_m=distance,
            cube_push_m=displacement,
            cube_contact_frames=self.contact_frames,
            furniture_contact_frames=len(self.bad_contacts),
            furniture_contacts=self.bad_contacts,
            independent_node_pids=sorted(set(nodes.values()) | self.pids),
            nodes=nodes,
            contact_audit_hz=self.info.get("contact_audit_hz", fps),
            root_pose_writes_during_rollout=0,
            dataset=str(self.dataset),
            raw=str(self.source),
            limitations=[
                "Rigid furniture, no sofa cushion deformation",
                "Solid-wheel collision meshes; forward/turn simulation calibration, not hardware omni-wheel validation",
            ],
        )
        if self.config.get("manual_recording"):
            result.update(success=n > 0, navigation_success=None, task_success=None,
                          recording_kind="manual teleoperation; task success not evaluated")
        write_json(self.root / "result.json", result)
        # Keep the actual commanded wheel rates, source physics clock and base pose.
        pq.write_table(
            pa.table(
                {
                    "sim_time_ns": [r["sim_time_ns"] for r in self.rows],
                    "base_pose": [r["base"] for r in self.rows],
                    "proposed_wheel_rates": [r["proposed"] for r in self.rows],
                    "applied_wheel_rates": [r["command"] for r in self.rows],
                }
            ),
            self.source / "control.parquet",
        )
        return result
