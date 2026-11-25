"""Portable acceptance checks for the Nano/Studio integration probe."""

import math
import unittest
from pathlib import Path

from nano_validation import load_model, motion_target, native_indices


MODEL = Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf"


class NanoContractTests(unittest.TestCase):
    def setUp(self):
        self.model = load_model(MODEL)

    def test_actual_nano_has_two_independent_six_channel_arms(self):
        self.assertEqual(len(self.model["names"]), 17)
        for side in ("left", "right"):
            self.assertEqual(len(self.model[side]), 6)
        self.assertFalse(set(self.model["left"]) & set(self.model["right"]))
        self.assertEqual(len(self.model["held"]), 5)

    def test_mapping_uses_names_even_if_engine_order_changes(self):
        names = self.model["names"]
        native = list(reversed(names))
        indices = native_indices(names, native)
        self.assertEqual([native[i] for i in indices], names)
        with self.assertRaises(ValueError):
            native_indices(names, native[:-1])

    def test_left_only_and_right_only_do_not_command_other_arm(self):
        for side in ("left", "right"):
            target = motion_target(self.model, side, 0.5)
            moving = {n for n, q in zip(self.model["names"], target) if abs(q) > 1e-8}
            self.assertEqual(moving, set(self.model[side]))

    def test_both_arms_move_and_head_and_wheels_hold(self):
        target = motion_target(self.model, "both", 0.5)
        moving = {n for n, q in zip(self.model["names"], target) if abs(q) > 1e-8}
        self.assertEqual(moving, set(self.model["left"] + self.model["right"]))

    def test_trajectories_start_finish_at_home_and_respect_actual_limits(self):
        for mode in ("left", "right", "both"):
            for phase in (0.0, 1.0):
                self.assertTrue(
                    all(abs(q) < 1e-8 for q in motion_target(self.model, mode, phase))
                )
            for i in range(101):
                for name, q in zip(
                    self.model["names"], motion_target(self.model, mode, i / 100)
                ):
                    low, high = self.model["limits"][name]
                    self.assertTrue(math.isfinite(q) and low <= q <= high)

    def test_invalid_phase_or_motion_is_refused(self):
        for phase in (-0.1, 1.1, float("nan"), float("inf")):
            with self.assertRaises(ValueError):
                motion_target(self.model, "both", phase)
        with self.assertRaises(ValueError):
            motion_target(self.model, "invalid", 0.5)


if __name__ == "__main__":
    unittest.main()
