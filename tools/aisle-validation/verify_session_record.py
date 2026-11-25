"""Verify a Studio teleoperation take against its lossless sensor archive."""

import argparse
import hashlib
import json
from pathlib import Path
import cv2
import numpy as np
import pyarrow.parquet as pq
from nano_living import CAMERAS


def verify(root):
    info = json.loads((root / "meta/info.json").read_text())
    capture = json.loads((root / "meta/capture.json").read_text())
    result = json.loads((root / "meta/capture-result.json").read_text())
    scene = json.loads((root / "meta/scene/episode_000000.json").read_text())
    rows = pq.read_table(root / "data/chunk-000/episode_000000.parquet").to_pylist()
    audit = [
        json.loads(line)
        for line in (root / "raw/frames.jsonl").read_text().splitlines()
    ]
    n = len(rows)
    assert n == info["total_frames"] == len(audit) == len(scene["frames"]) and n > 0
    assert result["task_success"] is None and result["success"]
    clocks = []
    for i, (row, checksum) in enumerate(zip(rows, audit)):
        path = root / f"raw/frame_{i:06d}.npz"
        assert hashlib.sha256(path.read_bytes()).hexdigest() == checksum["sha256"]
        with np.load(path, allow_pickle=False) as raw:
            assert np.allclose(
                row["observation.state"], raw["observation"][7:24], atol=1e-7
            )
            assert np.allclose(row["action"], raw["observation"][24:41], atol=1e-7)
            assert np.allclose(scene["frames"][i]["transforms"], raw["transforms"])
            assert int(raw["sim_time_ns"]) == (i + 1) * 200_000_000
            clocks.append(int(raw["source_sim_time_ns"]))
            assert raw["command_velocity"].shape == (17,)
            assert raw["physics_contacts"].shape[1] == 3
            for name in CAMERAS:
                spec = capture["cameras"][name]
                assert spec["fps"] == info["fps"] == 5
                assert "not measured hardware" in spec["optical_calibration"]
                k = np.array(spec["intrinsics"])
                assert k.shape == (3, 3) and k[0, 0] > 0 and k[1, 1] > 0
                assert raw[name + "__rgb"].shape == (480, 640, 3)
                assert (
                    raw[name + "__depth"].shape
                    == raw[name + "__seg"].shape
                    == (480, 640)
                )
                assert np.isfinite(raw[name + "__depth"]).all()
                assert int(raw[name + "__sim_time_ns"]) == int(raw["sim_time_ns"])
                assert np.allclose(
                    raw[name + "__world_from_camera_gl"],
                    raw[name + "__world_from_mount_link"]
                    @ np.array(spec["link_from_camera_gl"]),
                    atol=1e-5,
                )
    assert np.all(np.diff(clocks) == 200_000_000)
    counts = {}
    for name in CAMERAS:
        reader = cv2.VideoCapture(
            str(root / f"videos/chunk-000/observation.images.{name}/episode_000000.mp4")
        )
        count = 0
        while True:
            ok, frame = reader.read()
            if not ok:
                break
            assert frame.shape == (480, 640, 3)
            count += 1
        reader.release()
        assert count == n, (name, count, n)
        counts[name] = count
    return dict(
        passed=True,
        frames=n,
        camera_decoded_frames=counts,
        joints=17,
        raw_checksums_verified=n,
        camera_poses_verified=3 * n,
        intrinsics="nominal simulation values, preserved with provenance",
        source_clock="continuous 200 ms steps",
        task_success="not evaluated",
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    report = verify(args.dataset)
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
