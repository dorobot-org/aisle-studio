"""Independently verify Dora routing, turn barriers and received camera bytes."""

import hashlib
import json
from pathlib import Path
import sys

import yaml

from nano_dora import FrameBarrier, request_metadata, validate_graph
from nano_validation import digest, load_model, write_json


def verify(root):
    graph = yaml.safe_load((root / "dataflow.yaml").read_text())
    validate_graph(graph)
    config = json.loads((root / "dora-config.json").read_text())
    source = root / "aisle-run"
    proof = json.loads((source / "dora-recording.json").read_text())
    manifest = json.loads((source / "manifest.json").read_text())
    model = load_model(Path(config["model"]))
    for name, expected in config["sources"].items():
        assert digest(root / "node-source" / name) == expected, name
    assert digest(source / "dora-events.jsonl") == proof["events_sha256"]
    assert len(set(proof["node_pids"].values())) == 5
    assert proof["complete"] and proof["frames"] == 3 * config["frames"]
    rows = [
        json.loads(line)
        for line in (source / "dora-events.jsonl").read_text().splitlines()
    ]
    assert rows[0]["topic"] == "ready"
    request_id, barrier, pending, checked_cameras, checked_commands = 0, None, {}, 0, 0
    for row in rows[1:]:
        topic, meta = row["topic"], row["metadata"]
        expected = request_metadata(request_id, config["frames"], model)
        assert meta["request_id"] == request_id
        if barrier is None:
            required = {"proposed", "approved"} | (
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
                    "head",
                    "left_wrist",
                    "right_wrist",
                }
            )
            barrier = FrameBarrier(required, request_id)
        complete = barrier.add(topic, request_id)
        pending[topic] = row
        role = (
            "trajectory"
            if topic == "proposed"
            else "limits"
            if topic == "approved"
            else "sim"
        )
        assert meta[role + "_pid"] == proof["node_pids"][role]
        for name in (
            "episode",
            "frame",
            "mode",
            "op",
            "schema",
            "units",
            "joint_order",
        ):
            assert meta[name] == expected[name], (topic, name)
        if topic in ("head", "left_wrist", "right_wrist"):
            assert row["arrow_type"] == "uint8"
            spec = manifest["cameras"][topic]
            file = (
                source
                / f"traces/frames/{spec['source_directory']}/{meta['sim_time_ns']:020d}.npz"
            )
            assert (
                hashlib.sha256(file.read_bytes()).hexdigest() == row["payload_sha256"]
            )
            checked_cameras += 1
        if complete:
            assert (
                pending["proposed"]["payload_sha256"]
                == pending["approved"]["payload_sha256"]
            )
            assert (
                pending["approved"]["metadata"]["sim_time_ns"]
                == expected["sim_time_ns"]
            )
            if expected["op"] == "frame":
                assert (
                    pending["approved"]["payload_sha256"]
                    == pending["applied"]["payload_sha256"]
                )
                times = {
                    "gripper_applied": 10_000_000,
                    "joint_state": 20_000_000,
                    "objects": 20_000_000,
                    "head": 20_000_000,
                    "left_wrist": 20_000_000,
                    "right_wrist": 20_000_000,
                    "applied": 30_000_000,
                    "frame_done": 100_000_000,
                }
                for name, offset in times.items():
                    assert (
                        pending[name]["metadata"]["sim_time_ns"]
                        == expected["sim_time_ns"] + offset
                    )
                checked_commands += 1
            request_id += 1
            barrier, pending = None, {}
    assert (
        barrier is None
        and request_id == proof["acknowledged_turns"] == 3 * (config["frames"] + 1) + 1
    )
    assert (
        checked_commands == proof["frames"] and checked_cameras == 3 * proof["frames"]
    )
    assert proof["physics_steps"] == proof["frames"] * 10 + 3
    return dict(
        ok=True,
        separate_dora_processes=5,
        commands_checked=checked_commands,
        camera_payloads_checked=checked_cameras,
        complete_turns=request_id,
        routing_and_guard_enforced=True,
        no_dropped_or_duplicate_frames=True,
        received_camera_bytes_match_archive=True,
        approved_commands_match_applied=True,
        physics_steps=proof["physics_steps"],
        source_snapshot_verified=True,
    )


if __name__ == "__main__":
    root = Path(sys.argv[1])
    result = verify(root)
    write_json(root / "dora-verification.json", result)
    print(json.dumps(result))
