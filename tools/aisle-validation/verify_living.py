"""Verify every source sample, camera, base pose, command and playback export."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import cv2
import numpy as np
import pyarrow.parquet as pq
from nano_living import CAMERAS, MAX_WHEEL_RATE


def verify_ui(root, samples_path):
    """Compare saved Makepad instrument reports with the actual source samples."""
    info = json.loads((root / "scene-info.json").read_text())
    names = info["joint_names"]
    fps = info["fps"]
    n = json.loads((root / "result.json").read_text())["frames"]
    checks = []
    for report in json.loads(samples_path.read_text()):
        s = report["state"]
        live = s["live"]
        i = live["sequence"] if live else min(n - 1, math.floor(s["time"] * fps + 0.5))
        with np.load(root / f"raw/frame_{i:06d}.npz", allow_pickle=False) as raw:
            obs = raw["observation"]
            for key in ["recorded_joints", "viewer_joints"]:
                np.testing.assert_allclose(s[key], obs[7:24], atol=1e-6)
            np.testing.assert_allclose(
                s["robot_probe"]["scene_transforms"], raw["transforms"], atol=1e-6
            )
            np.testing.assert_allclose(s["base_pose"]["position"], obs[:3], atol=1e-6)
            matrix = np.asarray(s["robot_probe"]["base_transform"]).reshape(
                4, 4, order="F"
            )
            np.testing.assert_allclose(matrix[:3, 3], obs[:3], atol=1e-6)
            w, x, y, z = obs[3:7] / np.linalg.norm(obs[3:7])
            rotation = [
                [1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)],
                [2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)],
                [2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)],
            ]
            np.testing.assert_allclose(matrix[:3, :3], rotation, atol=2e-6)
            if live:
                assert live["connected"] and sorted(live["cameras"]) == sorted(CAMERAS)
                assert abs(s["time"] - (i + 1) / fps) < 1e-8
            for window in ["signals"] + (
                ["monitor_signals"] if s["monitor_open"] else []
            ):
                for lane in s[window]["lanes"]:
                    if not lane["visible"]:
                        continue
                    j = names.index(lane["name"])
                    actual, target = [
                        0.0 if abs(v) < 0.0005 else v for v in [obs[7 + j], obs[24 + j]]
                    ]
                    assert (
                        lane["readout"] == f"Actual {actual:.3f} · Target {target:.3f}"
                    )
                    assert abs(lane["plot"]["cursor_time"] - s["time"]) < 1e-8
                    if live:
                        np.testing.assert_allclose(
                            [v[1] for v in lane["plot"]["values"]],
                            [obs[7 + j], obs[24 + j]],
                            atol=1e-6,
                        )
            checks.append(
                dict(
                    frame=i,
                    live=bool(live),
                    joints=17,
                    scene_objects=len(info["objects"]),
                    rendered_base=True,
                    signals=True,
                )
            )
    assert len(checks) > 1
    result = dict(ok=True, samples=checks)
    samples_path.with_suffix(".verified.json").write_text(json.dumps(result, indent=2))
    return result


def verify(root):
    result = json.loads((root / "result.json").read_text())
    info = json.loads((root / "scene-info.json").read_text())
    config = json.loads((root / "config.json").read_text())
    n = result["frames"]
    fps = info["fps"]
    for name, digest in config["sources"].items():
        assert (
            hashlib.sha256((root / "node-source" / name).read_bytes()).hexdigest()
            == digest
        )
    audit = [
        json.loads(line)
        for line in (root / "raw/frames.jsonl").read_text().splitlines()
    ]
    parquet = pq.read_table(
        root / "lerobot/data/chunk-000/episode_000000.parquet"
    ).to_pylist()
    control = pq.read_table(root / "raw/control.parquet").to_pylist()
    scene = json.loads((root / "lerobot/meta/scene/episode_000000.json").read_text())
    assert scene["version"] == 2
    assert len(audit) == len(parquet) == len(control) == len(scene["frames"]) == n
    counts = {name: 0 for name in CAMERAS}
    previous = None
    for i, (a, row, c, f) in enumerate(zip(audit, parquet, control, scene["frames"])):
        path = root / f"raw/frame_{i:06d}.npz"
        assert hashlib.sha256(path.read_bytes()).hexdigest() == a["sha256"]
        assert a["frame"] == i and a["sim_time_ns"] == round((i + 1) / fps * 1e9)
        with np.load(path, allow_pickle=False) as source:
            obs = source["observation"]
            assert np.isfinite(obs).all()
            if info.get("contact_audit_hz") == 100:
                steps = round(100 / fps)
                assert np.array_equal(
                    source["physics_ticks"],
                    np.arange(i * steps + 1, (i + 1) * steps + 1),
                )
                contacts = source["physics_contacts"]
                assert contacts.ndim == 2 and contacts.shape[1] == 3
                if len(contacts):
                    assert contacts[:, 0].min() >= i * steps + 1
                    assert contacts[:, 0].max() <= (i + 1) * steps
                robots, furniture = (
                    set(info["robot_body_ids"]),
                    set(info["furniture_body_ids"]),
                )
                push = info["push_cube_body_id"]
                bad, pushing = [], False
                for _, a_body, b_body in contacts:
                    if (a_body in robots and b_body in furniture) or (
                        b_body in robots and a_body in furniture
                    ):
                        bad.append([a_body, b_body])
                    pushing |= (a_body in robots and b_body == push) or (
                        b_body in robots and a_body == push
                    )
                assert np.array_equal(
                    np.asarray(bad, np.int64).reshape(-1, 2), source["contacts"]
                )
                assert bool(source["cube_contact"][0]) == pushing
            assert np.array_equal(
                np.array(row["observation.state"], np.float32),
                obs[7:24].astype(np.float32),
            )
            assert np.array_equal(
                np.array(row["action"], np.float32), obs[24:41].astype(np.float32)
            )
            assert np.allclose(f["base_pose"]["position"], obs[:3], atol=1e-6)
            assert np.allclose(
                f["base_pose"]["quat_xyzw"], obs[[4, 5, 6, 3]], atol=1e-6
            )
            assert np.array_equal(
                np.array(f["transforms"], np.float32), source["transforms"]
            )
            assert (
                row["frame_index"] == i
                and abs(row["timestamp"] - i / fps) < 1e-8
                and abs(f["timestamp"] - row["timestamp"]) < 1e-8
            )
            command = source["command"]
            assert np.abs(command).max() <= MAX_WHEEL_RATE + 1e-6
            if previous is not None:
                assert np.abs(command - previous).max() <= 1.000001
            previous = command
            assert c["sim_time_ns"] == a["sim_time_ns"] and np.allclose(
                c["applied_wheel_rates"], command
            )
            for name in CAMERAS:
                assert int(source[f"{name}__sim_time_ns"]) == a["sim_time_ns"]
                assert source[f"{name}__rgb"].shape == (
                    config["height"],
                    config["width"],
                    3,
                )
                assert source[f"{name}__rgb"].dtype == np.uint8
                assert np.isfinite(source[f"{name}__depth"]).all()
                assert source[f"{name}__seg"].shape == (
                    config["height"],
                    config["width"],
                )
                if "segmentation_id_map" in info:
                    assert set(np.unique(source[f"{name}__seg"]).tolist()) <= {
                        int(k) for k in info["segmentation_id_map"]
                    }
                mount = source[f"{name}__world_from_mount_link"]
                camera = source[f"{name}__world_from_camera_gl"]
                offset = info["cameras"][name]["link_from_camera_gl"]
                assert np.allclose(camera, mount @ offset, atol=1e-5)
                counts[name] += 1
    video_checks = 0
    video_errors = []
    for name in CAMERAS:
        path = (
            root
            / f"lerobot/videos/chunk-000/observation.images.{name}/episode_000000.mp4"
        )
        video = cv2.VideoCapture(str(path))
        decoded = 0
        while True:
            ok, frame = video.read()
            if not ok:
                break
            assert frame.shape == (config["height"], config["width"], 3)
            decoded += 1
        assert decoded == n
        for index in [0, n // 2, n - 1, n // 4, 0]:
            video.set(cv2.CAP_PROP_POS_FRAMES, index)
            ok, frame = video.read()
            assert ok
            with np.load(
                root / f"raw/frame_{index:06d}.npz", allow_pickle=False
            ) as source:
                expected = cv2.cvtColor(source[f"{name}__rgb"], cv2.COLOR_RGB2BGR)
                error = float(
                    np.abs(
                        frame.astype(np.float32) - expected.astype(np.float32)
                    ).mean()
                )
                assert error < 10.0, (name, index, error)
                video_errors.append(error)
            video_checks += 1
        video.release()
    pids = set()
    for a in audit:
        pids.update(a["pids"].values())
    assert len(pids) >= 3
    if "nodes" in result:
        assert set(result["nodes"]) == {"nav", "guard", "sim", "recorder", "preview"}
        assert len(set(result["nodes"].values())) == 5
    assert (
        result["success"]
        and result["navigation_success"]
        and result["furniture_contact_frames"] == 0
    )
    assert result["cube_contact_frames"] > 0 and result["cube_push_m"] > 0.15
    report = dict(
        ok=True,
        frames=n,
        camera_frames=counts,
        all_raw_channels=True,
        source_hashes=True,
        base_and_scene_replay=True,
        wheel_limits=True,
        video_backward_seek_checks=video_checks,
        video_source_max_mean_pixel_error=max(video_errors),
        physics_navigation=result["navigation_success"],
        base_distance_m=result["base_distance_m"],
        cube_push_m=result["cube_push_m"],
        furniture_contact_frames=0,
        contact_audit_hz=info.get("contact_audit_hz", fps),
    )
    (root / "verification.json").write_text(json.dumps(report, indent=2))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--ui-samples", type=Path)
    args = parser.parse_args()
    print(
        json.dumps(
            verify_ui(args.run, args.ui_samples)
            if args.ui_samples
            else verify(args.run)
        )
    )
