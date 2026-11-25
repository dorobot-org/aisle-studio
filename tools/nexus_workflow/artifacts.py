"""Validate, stage, and read the LeRobot v3 datasets used by Nexus jobs."""

import math
import os
from pathlib import Path
import sys

from workflow_io import (
    FPS, HEIGHT, JOINTS, RGB_BYTES, WIDTH, in_lerobot_units, integer, read_json, vector, write_json,
)

STATE, ACTION, CAMERA = "observation.state", "action", "observation.images.front"
REPO_ID = "local/so101_nexus_cube_to_plate"
JOINT_FEATURE_NAMES = [f"{name}.pos" for name in JOINTS]


def dataset_info(directory):
    path = directory / "meta/info.json"
    metadata = read_json(path)
    if (
        not isinstance(metadata, dict) or not str(metadata.get("codebase_version", "")).startswith("v3.")
        or metadata.get("fps") != FPS
    ):
        raise ValueError(f"{directory} must be a LeRobot v3 dataset recorded at {FPS} Hz")
    integer(metadata.get("total_episodes"), f"episode count in {path}", minimum=1)
    features = metadata.get("features", {})
    if not isinstance(features, dict) or any(not isinstance(value, dict) for value in features.values()):
        raise ValueError(f"Invalid LeRobot features in {path}")
    for name in (STATE, ACTION):
        expected = {"dtype": "float32", "shape": [6], "names": JOINT_FEATURE_NAMES}
        feature = features.get(name, {})
        if any(feature.get(key) != value for key, value in expected.items()):
            raise ValueError(f"{directory} must contain {name} for the six named SO-101 axes")
    camera = features.get(CAMERA, {})
    if camera.get("dtype") != "video" or camera.get("shape") != [HEIGHT, WIDTH, 3]:
        raise ValueError(f"{directory} must contain {WIDTH}×{HEIGHT} RGB video at {CAMERA}")
    if any(name != CAMERA and value.get("dtype") in ("video", "image") for name, value in features.items()):
        raise ValueError(f"{directory} must contain one front camera")
    return metadata


def sources(selected):
    if not selected.is_dir():
        raise ValueError(f"Choose a local LeRobot v3 dataset or a directory of datasets: {selected}")

    roots = []
    for root, dirs, _ in os.walk(selected):
        directory = Path(root)
        if (directory / "meta/info.json").is_file():
            roots.append(directory)
            dirs.clear()
        else:
            derived = (directory / "dataset-sources.json").is_file()
            dirs[:] = sorted(
                name for name in dirs if not name.startswith(".") and not (derived and name == "training-dataset")
            )
    if not roots:
        raise ValueError(f"No LeRobot v3 datasets found in {selected}")

    entries = []
    for root in roots:
        info = dataset_info(root)
        exclusion = root / "meta/dorobot_excluded.json"
        excluded = read_json(exclusion) if exclusion.exists() else []
        if not isinstance(excluded, list) or any(
            type(index) is not int or not 0 <= index < info["total_episodes"] for index in excluded
        ):
            raise ValueError(f"Invalid excluded episode indices in {exclusion}")
        blocked = set(excluded)
        entry = {
            "path": str(root), "episodes": [i for i in range(info["total_episodes"]) if i not in blocked],
            "excluded": sorted(blocked),
        }
        capture_metadata = root / "meta/dorobot_capture.json"
        if capture_metadata.is_file():
            entry["source_mode"] = read_json(capture_metadata)["source_mode"]
        entries.append(entry)
    if not any(entry["episodes"] for entry in entries):
        raise ValueError(f"All episodes in {selected} have been excluded")
    return entries


def open_dataset(directory):
    from lerobot.datasets import LeRobotDataset

    dataset_info(directory)
    for pattern in ("meta/episodes/**/*.parquet", "data/**/*.parquet", f"videos/{CAMERA}/**/*.mp4"):
        if not any(directory.glob(pattern)):
            raise ValueError(f"Dataset {directory} is missing files matching {pattern}")
    return LeRobotDataset(repo_id=REPO_ID, root=directory, video_backend="pyav")


def export(take):
    import numpy as np
    from lerobot.configs.video import RGBEncoderConfig
    from lerobot.datasets import LeRobotDataset

    trace = read_json(take / "trace.json")
    frames = trace["frames"]
    raw = take / "frames.rgb"
    limits = vector(trace["gripper_limits_rad"], 2, "gripper limits")
    if raw.stat().st_size != len(frames) * RGB_BYTES:
        raise ValueError("Take RGB file does not contain every synchronized frame")
    directory = take / "dataset"
    dataset = LeRobotDataset.create(
        repo_id=REPO_ID, root=directory, robot_type="so101_follower", fps=FPS,
        features={
            STATE: {"dtype": "float32", "shape": (6,), "names": JOINT_FEATURE_NAMES},
            ACTION: {"dtype": "float32", "shape": (6,), "names": JOINT_FEATURE_NAMES},
            CAMERA: {"dtype": "video", "shape": (HEIGHT, WIDTH, 3), "names": ["height", "width", "channels"]},
        },
        rgb_encoder=RGBEncoderConfig(vcodec="h264"), video_backend="pyav", encoder_threads=2,
    )
    images = np.memmap(raw, mode="r", dtype=np.uint8, shape=(len(frames), HEIGHT, WIDTH, 3))
    for frame, rgb in zip(frames, images, strict=True):
        dataset.add_frame({
            "task": trace["task"], CAMERA: rgb,
            STATE: np.asarray(in_lerobot_units(frame["observation_state_rad"], limits), dtype=np.float32),
            ACTION: np.asarray(in_lerobot_units(frame["accepted_target_rad"], limits), dtype=np.float32),
        })
    dataset.save_episode(parallel_encoding=False)
    dataset.finalize()

    reopened = open_dataset(directory)
    if reopened.num_episodes != 1 or len(reopened) != len(frames):
        raise ValueError("LeRobot v3 readback did not match the staged episode length")
    for index in (0, len(frames) - 1):
        row, frame = reopened[index], frames[index]
        if list(row[CAMERA].shape) != [3, HEIGHT, WIDTH] or row["task"] != trace["task"]:
            raise ValueError("LeRobot v3 readback did not match the staged task and camera")
        if not math.isclose(row["timestamp"].item(), frame["timestamp"], abs_tol=1e-5):
            raise ValueError("LeRobot v3 readback did not preserve the 20 Hz sample timestamps")
        np.testing.assert_allclose(row[STATE], in_lerobot_units(frame["observation_state_rad"], limits), atol=1e-5)
        np.testing.assert_allclose(row[ACTION], in_lerobot_units(frame["accepted_target_rad"], limits), atol=1e-5)
    write_json(directory / "meta/dorobot_capture.json", {
        "source_mode": trace["source_mode"], "sample_hz_simulated": FPS,
    })
    print(f"Verified {len(reopened)} synchronized frames in a local LeRobot v3 dataset", flush=True)


def prepare(selected, stage):
    from lerobot.datasets.dataset_tools import delete_episodes, merge_datasets

    entries = sources(selected)
    expected_episodes = 0
    datasets = []
    for index, entry in enumerate(entries):
        if not entry["episodes"]:
            continue
        expected_episodes += len(entry["episodes"])
        dataset = open_dataset(Path(entry["path"]))
        if entry["excluded"]:
            dataset = delete_episodes(
                dataset, entry["excluded"], output_dir=stage / f"source-{index:03}", repo_id=REPO_ID,
            )
        datasets.append(dataset)

    expected_frames = sum(len(dataset) for dataset in datasets)
    if len(datasets) > 1:
        merged = merge_datasets(datasets, REPO_ID, output_dir=stage / "dataset",
                                concatenate_videos=False, concatenate_data=False)
        source = merged.root
    elif datasets[0].root.is_relative_to(stage):
        dataset_dir = stage / "dataset"
        datasets[0].root.rename(dataset_dir)
        source = dataset_dir
    else:
        source = datasets[0].root

    readback = open_dataset(source)
    if readback.num_episodes != expected_episodes or len(readback) != expected_frames:
        raise ValueError("Selected LeRobot episodes did not match dataset readback")
    first = readback[0]
    last = readback[len(readback) - 1]
    for row in (first, last):
        if list(row[CAMERA].shape) != [3, HEIGHT, WIDTH]:
            raise ValueError("Selected LeRobot front video is incompatible with SO-101 ACT")
    write_json(stage / "dataset-sources.json", {
        "sources": entries, "total_episodes": expected_episodes, "total_frames": expected_frames,
        "initial_state_deg_percent": vector(first[STATE].tolist(), 6, "initial state"),
        "artifact": "dataset" if source.is_relative_to(stage) else str(source),
    })


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "export":
        export(Path(sys.argv[2]))
    elif len(sys.argv) == 4 and sys.argv[1] == "prepare":
        prepare(Path(sys.argv[2]), Path(sys.argv[3]))
    else:
        sys.exit("Usage: artifacts.py export TAKE | prepare SOURCE STAGING_DIRECTORY")
