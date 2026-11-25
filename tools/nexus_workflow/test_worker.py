"""Regression checks for Studio's SO-101 worker and artifact contracts."""
import io
import json
from pathlib import Path
import struct
from tempfile import TemporaryDirectory
import unittest

import artifacts
import capture
import policy
from workflow_io import PreviewWriter, Protocol, read_frame


RGB = bytes(640 * 480 * 3)


def packet(seq, reset_id=0, paused=False, tick=None, leader=True):
    return {
        "seq": seq, "reset_id": reset_id, "paused": paused,
        "sim_tick": seq * 5 if tick is None else tick,
        "capture_monotonic_ns": seq * 50_000_000,
        "leader_deg_percent": [0.0] * 6 if leader else None,
        "state_rad": [0.0] * 6, "target_rad": [0.05] * 6,
        "gripper_limits_rad": [-0.2, 1.2], "cube_xyz": [0.0, 0.0, 0.04],
        "plate_xyz": [0.25, 0.0, 0.025], "rgb_bytes": len(RGB),
    }


class CaptureTest(unittest.TestCase):
    def setUp(self):
        self.tmp = TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.protocol = Protocol(self.root)
        self.session = capture.Capture(self.protocol, self.root, source_mode="synthetic")
        self.addCleanup(self.session.close)

    def send(self, command_id, action):
        (self.root / "command.json").write_text(
            json.dumps({"version": 1, "id": command_id, "action": action})
        )
        command = self.protocol.command(self.session.pending_id)
        self.assertIsNotNone(command)
        self.session.command(*command)
        return json.loads((self.root / "status.json").read_text())

    def advance(self, seq, reset_id=0, **kwargs):
        self.session.frame(packet(seq, reset_id, **kwargs), RGB)
        return json.loads((self.root / "status.json").read_text())

    def ready(self):
        self.advance(0)
        self.send(1, "reset")
        for seq in range(1, 9):
            self.advance(seq, 1)

    def test_record_requires_runtime_reset_confirmation_and_fresh_frames(self):
        self.assertEqual(self.advance(0)["phase"], "reset_required")
        self.assertEqual(self.send(1, "record")["ack"], 1)
        resetting = self.send(2, "reset")
        self.assertEqual(resetting["phase"], "resetting")
        self.assertEqual(resetting["ack"], 1)
        for seq in range(1, 12):
            self.assertEqual(self.advance(seq)["phase"], "resetting")
        self.advance(12, 1, leader=False)
        for seq in range(13, 20):
            self.assertEqual(self.advance(seq, 1)["phase"], "resetting")
        ready = self.advance(20, 1)
        self.assertEqual((ready["phase"], ready["ack"]), ("ready", 2))

    def test_finish_stages_synced_frames_until_explicit_discard(self):
        self.ready()
        self.assertEqual(self.send(2, "record")["phase"], "recording")
        for seq in range(9, 17):
            self.advance(seq, 1)
        review = self.send(3, "finish")
        self.assertEqual((review["phase"], review["frames"]), ("review", 8))
        self.assertNotIn("dataset", review)
        self.assertEqual(review["source_mode"], "synthetic")
        self.assertFalse(list(self.root.glob("*/dataset/meta/info.json")))
        trace_file, = self.root.glob(".episode-*/trace.json")
        trace = json.loads(trace_file.read_text())
        self.assertEqual(trace["source_mode"], "synthetic")
        self.assertEqual([sample["timestamp"] for sample in trace["frames"]], [i / 20 for i in range(8)])
        self.assertEqual(trace["frames"][0]["accepted_target_rad"], [0.05] * 6)
        self.assertEqual(trace["frames"][0]["observation_state_rad"], [0.0] * 6)
        self.assertEqual(self.send(4, "discard")["phase"], "reset_required")
        self.assertFalse(trace_file.exists())

    def test_pause_and_step_acknowledge_observed_simulator_state(self):
        self.ready()
        self.assertEqual(self.send(2, "pause")["ack"], 1)
        self.assertEqual(self.advance(9, 1, paused=True, tick=40)["phase"], "paused")
        self.assertEqual(self.send(3, "step")["ack"], 2)
        self.assertEqual(self.advance(10, 1, paused=True, tick=40)["ack"], 2)
        self.assertEqual(self.advance(11, 1, paused=True, tick=41)["ack"], 3)
        self.assertEqual(self.send(4, "resume")["ack"], 3)
        self.assertEqual(self.advance(12, 1, tick=46)["phase"], "ready")

    def test_older_pending_ack_cannot_replace_newer_invalid_stop_ack(self):
        self.ready()
        self.send(2, "pause")
        (self.root / "command.json").write_text(json.dumps({"version": 2, "id": 3, "action": "stop"}))
        self.assertIsNone(self.protocol.command(self.session.pending_id))
        paused = self.advance(9, 1, paused=True, tick=40)
        self.assertEqual((paused["phase"], paused["ack"]), ("paused", 3))

    def test_missing_capture_packet_rejects_partial_take(self):
        self.ready()
        self.send(2, "record")
        self.advance(9, 1)
        rejected = self.advance(11, 1)
        self.assertEqual(rejected["phase"], "reset_required")
        self.assertFalse(list(self.root.glob(".episode-*")))

    def test_pending_capture_command_accepts_stop_override(self):
        self.ready()
        self.send(2, "pause")
        self.assertIsNone(self.protocol.command(self.session.pending_id))
        stopping = self.send(3, "stop")
        self.assertEqual((stopping["phase"], stopping["ack"]), ("stopping", 3))


class ProtocolTest(unittest.TestCase):
    def test_preview_is_big_endian_with_packed_rgb(self):
        with TemporaryDirectory() as output:
            root = Path(output)
            protocol = Protocol(root)
            protocol.write_preview(257, RGB)
            data = (root / "preview.rgb").read_bytes()
            self.assertEqual(data[:12], struct.pack(">QHH", 257, 640, 480))
            self.assertEqual(data[12:], RGB)

    def test_preview_writer_flushes_the_final_frame(self):
        with TemporaryDirectory() as output:
            root = Path(output)
            protocol = Protocol(root)
            with PreviewWriter(protocol) as preview:
                preview.offer((42, RGB))
            self.assertEqual(struct.unpack(">QHH", (root / "preview.rgb").read_bytes()[:12])[0], 42)

    def test_saved_leader_allows_an_explicit_calibration_file(self):
        with TemporaryDirectory() as output:
            root = Path(output)
            (root / "leader.json").touch()
            selected = root / "so101_leader.json"
            selected.touch()
            self.assertEqual(capture.leader_calibration(str(selected)), (root, "so101_leader"))
            self.assertEqual(capture.leader_calibration(str(root)), (root, "leader"))
            (root / "leader.json").unlink()
            self.assertEqual(capture.leader_calibration(str(root)), (root, "so101_leader"))

    def test_packet_reader_rejects_truncation_and_bad_rgb_size(self):
        header = packet(1)
        encoded = json.dumps(header).encode() + b"\n"
        self.assertEqual(read_frame(io.BytesIO(encoded + RGB)), (header, RGB))
        with self.assertRaisesRegex(ValueError, "truncated"):
            read_frame(io.BytesIO(encoded + RGB[:-1]))
        header["rgb_bytes"] = 48
        with self.assertRaisesRegex(ValueError, "RGB"):
            read_frame(io.BytesIO(json.dumps(header).encode() + b"\n" + RGB))


class ArtifactTest(unittest.TestCase):
    def test_source_selection_excludes_only_specified_episodes(self):
        with TemporaryDirectory() as output:
            root = Path(output)
            source = root / "sample" / "dataset"
            (source / "meta").mkdir(parents=True)
            info = {
                "codebase_version": "v3.0", "fps": 20, "total_episodes": 4,
                "features": {
                    "action": {"dtype": "float32", "shape": [6], "names": [
                        "shoulder_pan.pos", "shoulder_lift.pos", "elbow_flex.pos",
                        "wrist_flex.pos", "wrist_roll.pos", "gripper.pos",
                    ]},
                    "observation.images.front": {"dtype": "video", "shape": [480, 640, 3]},
                },
            }
            info["features"]["observation.state"] = info["features"]["action"]
            (source / "meta/info.json").write_text(json.dumps(info))
            (source / "meta/dorobot_excluded.json").write_text("[1, 3]")
            self.assertEqual(artifacts.sources(root), [
                {"path": str(source), "episodes": [0, 2], "excluded": [1, 3]}
            ])
            (source / "meta/dorobot_excluded.json").write_text("[0, 1, 2, 3]")
            with self.assertRaisesRegex(ValueError, "excluded"):
                artifacts.sources(root)

    def test_collection_discovery_skips_derived_training_datasets(self):
        with TemporaryDirectory() as output:
            root = Path(output)
            names = ["shoulder_pan.pos", "shoulder_lift.pos", "elbow_flex.pos",
                     "wrist_flex.pos", "wrist_roll.pos", "gripper.pos"]
            joint_feature = {"dtype": "float32", "shape": [6], "names": names}
            info = {"codebase_version": "v3.0", "fps": 20, "total_episodes": 2,
                    "features": {"observation.state": joint_feature, "action": joint_feature,
                                 "observation.images.front": {"dtype": "video", "shape": [480, 640, 3]}}}
            original = root / "capture-a" / "episode-001" / "dataset"
            training = root / "train-a"
            derived = training / "training-dataset"
            for dataset in (original, derived):
                (dataset / "meta").mkdir(parents=True)
                (dataset / "meta/info.json").write_text(json.dumps(info))
            (original / "meta/dorobot_excluded.json").write_text("[1]")
            (training / "dataset-sources.json").write_text(json.dumps({"artifact": str(derived)}))
            self.assertEqual(artifacts.sources(root), [
                {"path": str(original), "episodes": [0], "excluded": [1]},
            ])
            self.assertEqual(artifacts.sources(derived), [
                {"path": str(derived), "episodes": [0, 1], "excluded": []},
            ])

    def test_external_checkpoint_uses_default_pose_independent_of_training_selection(self):
        with TemporaryDirectory() as output:
            root = Path(output)
            initial = policy.rollout_initial({"dataset": str(root / "missing-dataset")}, root)
            self.assertEqual(initial, [0.0] * 5 + [0.6])

    def test_checkpoint_front_camera_shape_is_channel_first(self):
        config = {
            "type": "act", "n_obs_steps": 1,
            "input_features": {
                "observation.state": {"type": "STATE", "shape": [6]},
                "observation.images.front": {"type": "VISUAL", "shape": [480, 640, 3]},
            },
            "output_features": {"action": {"type": "ACTION", "shape": [6]}},
        }
        with self.assertRaisesRegex(ValueError, "observation.images.front"):
            policy.check_policy_features(config)
        config["input_features"]["observation.images.front"]["shape"] = [3, 480, 640]
        policy.check_policy_features(config)

    def test_training_progress_prefers_exact_counts_over_rounded_metrics(self):
        line = "Training:  66%|xyz| 1019/1500 [00:52] INFO step:1K smpl:8K loss:3.125"
        self.assertEqual(policy.training_progress(line, 1500), (1019, 3.125))
        self.assertEqual(policy.training_progress("INFO step:7 loss:1.234", 20), (7, 1.234))
        self.assertEqual(policy.training_progress("INFO step:1K loss:1.234", 1500), (None, 1.234))
        self.assertEqual(policy.training_progress("INFO train_config.json", 1500), (None, None))


if __name__ == "__main__":
    unittest.main()
