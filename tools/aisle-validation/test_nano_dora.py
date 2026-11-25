"""Dora migration: command admission, ordering and lossless frame barriers."""

import math
import unittest
from pathlib import Path

from nano_validation import load_model
from nano_dora import CommandGate, FrameBarrier, request_metadata, validate_graph


MODEL = Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf"


class DoraContractTests(unittest.TestCase):
    def setUp(self):
        self.model = load_model(MODEL)
        self.gate = CommandGate(self.model, 100)

    def meta(self, request_id):
        return request_metadata(request_id, 100, self.model)

    def test_reset_then_frames_and_no_duplicate_or_skipped_commands(self):
        self.gate.accept([0.0] * 17, self.meta(0))
        with self.assertRaises(ValueError):
            self.gate.accept([0.0] * 17, self.meta(0))
        with self.assertRaises(ValueError):
            self.gate.accept([0.0] * 17, self.meta(2))
        self.gate.accept([0.0] * 17, self.meta(1))

    def test_schema_order_units_and_finite_width_are_enforced(self):
        for key, value in [
            ("units", "degrees"),
            ("joint_order", "wrong"),
            ("schema", "unknown"),
            ("episode", 2),
        ]:
            meta = dict(self.meta(0), **{key: value})
            with self.assertRaises(ValueError):
                self.gate.accept([0.0] * 17, meta)
        for values in [[0.0] * 16, [math.nan] * 17, [math.inf] * 17]:
            with self.assertRaises(ValueError):
                self.gate.accept(values, self.meta(0))
        self.assertEqual(self.gate.next_id, 0)

    def test_guard_checks_limits_velocity_and_held_joints(self):
        self.gate.accept([0.0] * 17, self.meta(0))
        arm = self.model["names"].index("left_j1_yaw")
        for index, value in [(arm, 9.0), (arm, 1.0), (0, 0.1)]:
            values = [0.0] * 17
            values[index] = value
            with self.assertRaises(ValueError):
                self.gate.accept(values, self.meta(1))
        values = [0.0] * 17
        values[arm] = 0.01
        self.gate.accept(values, self.meta(1))

    def test_frame_barrier_requires_each_camera_and_rejects_duplicates(self):
        barrier = FrameBarrier(
            {"approved", "state", "head", "left", "right", "done"}, 7
        )
        for channel in ["done", "state", "right", "approved", "head"]:
            self.assertFalse(barrier.add(channel, 7))
        with self.assertRaises(ValueError):
            barrier.add("head", 7)
        with self.assertRaises(ValueError):
            barrier.add("left", 8)
        self.assertTrue(barrier.add("left", 7))

    def test_graph_cannot_route_controller_straight_to_simulator(self):
        import yaml

        graph = yaml.safe_load(Path(__file__).with_name("nano_dora.yaml").read_text())
        validate_graph(graph)
        sim = next(n for n in graph["nodes"] if n["id"] == "nano_sim")
        sim["inputs"]["command"]["source"] = "nano_trajectory/joint_cmd"
        with self.assertRaises(ValueError):
            validate_graph(graph)


if __name__ == "__main__":
    unittest.main()
