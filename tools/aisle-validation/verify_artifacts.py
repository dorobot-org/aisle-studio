"""Check converted rows and video against independent original AISLE traces."""

import json
import hashlib
import xml.etree.ElementTree as ET
from pathlib import Path

import cv2
import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq


def link_poses(urdf, names, values):
    q = dict(zip(names, values))
    transforms = {"base_link": np.eye(4)}
    for joint in urdf.findall("joint"):
        origin = joint.find("origin")
        r, p, y = map(float, origin.get("rpy", "0 0 0").split())
        c, s = np.cos, np.sin
        rx = np.array([[1, 0, 0], [0, c(r), -s(r)], [0, s(r), c(r)]])
        ry = np.array([[c(p), 0, s(p)], [0, 1, 0], [-s(p), 0, c(p)]])
        rz = np.array([[c(y), -s(y), 0], [s(y), c(y), 0], [0, 0, 1]])
        local = np.eye(4)
        local[:3, :3] = rz @ ry @ rx
        local[:3, 3] = list(map(float, origin.get("xyz").split()))
        if joint.get("type") != "fixed":
            axis = np.array(list(map(float, joint.find("axis").get("xyz").split())))
            axis /= np.linalg.norm(axis)
            x, y, z = axis
            cross = np.array([[0, -z, y], [z, 0, -x], [-y, x, 0]])
            angle = q[joint.get("name")]
            local[:3, :3] = local[:3, :3] @ (
                np.eye(3)
                + np.sin(angle) * cross
                + (1 - np.cos(angle)) * (cross @ cross)
            )
        transforms[joint.find("child").get("link")] = (
            transforms[joint.find("parent").get("link")] @ local
        )
    return transforms


def verify(root):
    source = root / "aisle-run"
    dataset = root / "lerobot"
    provenance = json.loads((dataset / "meta/aisle_provenance.json").read_text())
    manifest = json.loads((source / "manifest.json").read_text())
    info = json.loads((dataset / "meta/info.json").read_text())
    cameras = manifest.get(
        "cameras",
        {
            "front": dict(
                video_key="observation.images.front",
                source_directory="overhead",
                resolution=[320, 240],
            )
        },
    )
    for relative, expected in provenance.get("raw_recording_sha256", {}).items():
        assert (
            hashlib.sha256((source / relative).read_bytes()).hexdigest() == expected
        ), relative
    urdf = ET.parse(manifest["source_model"]).getroot()
    names = manifest["state_joint_names"]
    assert info["features"]["observation.state"]["names"] == names
    assert info["features"]["action"]["names"] == names

    def stream(topic):
        with pa.memory_map(str(source / "traces" / topic), "r") as file:
            return pa.ipc.open_stream(file).read_all().to_pylist()

    commands = stream(
        manifest.get("command_stream", "nano_limits__joint_cmd_safe.arrow")
    )
    observations = {
        r["sim_time_ns"]: r["data"] for r in stream("nano_state__joint_state.arrow")
    }
    mappings = provenance["mapping"]
    count, max_pixel_mae = 0, 0.0
    scene_rows = {}
    if manifest.get("scene_objects"):
        scene_rows = {
            r["sim_time_ns"]: json.loads(r["text"])
            for r in stream("nano_scene__objects.arrow")
        }
    scene_frames_checked = 0
    camera_counts = dict.fromkeys(cameras, 0)
    max_pose_error = 0.0
    max_fk_residual = 0.0
    for name, spec in cameras.items():
        assert (
            len(
                list(
                    (source / "traces/frames" / spec["source_directory"]).glob("*.npz")
                )
            )
            == info["total_frames"]
        ), name
    for episode in range(info["total_episodes"]):
        table = pq.read_table(
            dataset / f"data/chunk-000/episode_{episode:06d}.parquet"
        ).to_pylist()
        scene = None
        if scene_rows:
            scene = json.loads(
                (dataset / f"meta/scene/episode_{episode:06d}.json").read_text()
            )
            assert scene["objects"] == manifest["scene_objects"]
            assert len(scene["frames"]) == len(table)
        caps = {
            name: cv2.VideoCapture(
                str(
                    dataset
                    / f"videos/chunk-000/{spec['video_key']}/episode_{episode:06d}.mp4"
                )
            )
            for name, spec in cameras.items()
        }
        for frame_index, row in enumerate(table):
            mapping = mappings[count]
            command = commands[count]
            assert (mapping["episode"], mapping["frame"]) == (episode, frame_index)
            assert (
                mapping["camera_sim_time_ns"]
                == mapping["observation_sim_time_ns"]
                < command["sim_time_ns"]
            )
            assert mapping["command_sim_time_ns"] == command["sim_time_ns"]
            np.testing.assert_allclose(
                row["action"], command["data"], rtol=0, atol=1e-7
            )
            np.testing.assert_array_equal(
                row["observation.state"],
                observations[mapping["observation_sim_time_ns"]],
            )
            assert row["episode_index"] == episode and row["frame_index"] == frame_index
            assert row["index"] == count
            assert abs(row["timestamp"] - frame_index / 10) < 1e-9
            if scene:
                scene_frame = scene["frames"][frame_index]
                ts = mapping["camera_sim_time_ns"]
                assert scene_frame["sim_time_ns"] == ts
                assert scene_frame["timestamp"] == row["timestamp"]
                assert scene_frame["transforms"] == scene_rows[ts]["transforms"]
                scene_frames_checked += 1
            poses = link_poses(urdf, names, row["observation.state"])
            rgb_hashes = []
            for name, spec in cameras.items():
                ok, image = caps[name].read()
                assert ok, (episode, frame_index, name)
                ts = mapping["camera_sim_time_ns"]
                raw_path = (
                    source / f"traces/frames/{spec['source_directory']}/{ts:020d}.npz"
                )
                with np.load(raw_path, allow_pickle=False) as raw:
                    w, h = spec["resolution"]
                    assert raw["rgb"].shape == (h, w, 3)
                    rgb_hashes.append(hashlib.sha256(raw["rgb"].tobytes()).hexdigest())
                    error = np.abs(
                        cv2.cvtColor(image, cv2.COLOR_BGR2RGB).astype(float)
                        - raw["rgb"]
                    ).mean()
                    assert error < 8.0, (name, error)
                    max_pixel_mae = max(max_pixel_mae, float(error))
                    if "parent_link" in spec:
                        assert (
                            int(raw["sim_time_ns"])
                            == ts
                            == mapping["cameras"][name]["sim_time_ns"]
                        )
                        assert (
                            hashlib.sha256(raw_path.read_bytes()).hexdigest()
                            == mapping["cameras"][name]["sha256"]
                        )
                        assert raw["depth"].shape == raw["seg"].shape == (h, w)
                        assert (
                            raw["depth"].dtype == np.float32
                            and np.isfinite(raw["depth"]).all()
                        )
                        assert raw["seg"].dtype == np.int64
                        expected_pose = poses[spec["parent_link"]] @ np.asarray(
                            spec["link_from_camera_gl"]
                        )
                        # Compare the separately exposed joint and rendered-link readbacks.
                        # Preserve the actual render pose; report the nominal FK residual rather
                        # than replacing it with a fictitious ideal pose.
                        max_fk_residual = max(
                            max_fk_residual,
                            float(
                                np.max(
                                    np.abs(expected_pose - raw["world_from_camera_gl"])
                                )
                            ),
                        )
                        expected_pose = raw["world_from_mount_link"] @ np.asarray(
                            spec["link_from_camera_gl"]
                        )
                        pose_error = float(
                            np.max(np.abs(expected_pose - raw["world_from_camera_gl"]))
                        )
                        assert pose_error < 1e-5, (name, pose_error)
                        max_pose_error = max(max_pose_error, pose_error)
                camera_counts[name] += 1
            assert len(set(rgb_hashes)) == len(cameras), "Duplicate camera images"
            count += 1
        for cap in caps.values():
            assert not cap.read()[0]
            cap.release()
    assert count == len(commands) == info["total_frames"]
    return dict(
        ok=True,
        checked_frames=count,
        scene_frames_checked=scene_frames_checked,
        camera_frames=camera_counts,
        total_camera_frames=sum(camera_counts.values()),
        maximum_camera_attachment_error=max_pose_error,
        maximum_nominal_urdf_fk_residual=max_fk_residual,
        raw_archive_hashes_verified=bool(provenance.get("raw_recording_sha256")),
        camera_streams_distinct=True,
        joint_values_and_order_preserved=True,
        original_timestamps_preserved_in_provenance=True,
        no_future_frame_joins=True,
        video_frame_count_matches=True,
        maximum_video_rgb_mae=max_pixel_mae,
    )


if __name__ == "__main__":
    import sys

    root = Path(sys.argv[1])
    result = verify(root)
    (root / "artifact-verification.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print(json.dumps(result))
