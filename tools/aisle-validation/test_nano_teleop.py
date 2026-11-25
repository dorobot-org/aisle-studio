import unittest
from pathlib import Path
import numpy as np
from nano_teleop import (
    TeleopMapper,
    CommandGuard,
    active_velocity,
    integrate_targets,
    model_joints,
    TIMEOUT_NS,
)

MODEL = Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf"


class TeleopTests(unittest.TestCase):
    def setUp(self):
        self.names, self.limits = model_joints(MODEL)
        self.mapper = TeleopMapper(self.names)
        self.seq = 0

    def sample(self, buttons=None, axes=None, connected=True):
        self.seq += 1
        return self.mapper.command(
            dict(
                sequence=self.seq,
                input_ns=self.seq * 20_000_000,
                connected=connected,
                buttons=buttons or {},
                axes=axes or {},
            )
        )

    def test_connect_while_enabled_requires_release_and_neutral(self):
        self.assertFalse(self.sample({"enable": True}, {"left_y": -1})["enabled"])
        self.assertFalse(self.sample({}, {"left_y": -1})["enabled"])
        self.sample()
        cmd = self.sample({"enable": True}, {"left_y": -1})
        self.assertTrue(cmd["enabled"])
        self.assertGreater(np.linalg.norm(cmd["velocities"]), 0)
        self.assertFalse(self.sample()["enabled"])

    def test_button_only_gripper_and_both_arms_named_correctly(self):
        self.sample()
        for side in ("left", "right"):
            self.sample({"mode": True})
            self.sample()
            cmd = self.sample({"enable": True, "close": True})
            nonzero = [n for n, v in zip(self.names, cmd["velocities"]) if v]
            self.assertEqual(nonzero, [side + "_j6_gripper"])
            self.sample()

    def test_all_six_controls_route_by_name_on_both_arms(self):
        cases = [
            ("j1_yaw", {"left_x": 1}, {}, .3),
            ("j2_pitch", {"left_y": -1}, {}, .3),
            ("j3_pitch", {"right_y": -1}, {}, .3),
            ("j4_pitch", {"right_x": 1}, {}, .3),
            ("j5_roll", {}, {"right": True}, .3),
            ("j5_roll", {}, {"left": True}, -.3),
            ("j6_gripper", {}, {"close": True}, .5),
            ("j6_gripper", {}, {"open": True}, -.5),
        ]
        for side, mode in (("left", 1), ("right", 2)):
            for names in (self.names, list(reversed(self.names))):
                self.mapper = TeleopMapper(names)
                self.sample()
                for _ in range(mode):
                    self.sample({"mode": True})
                    self.sample()
                for suffix, axes, buttons, expected in cases:
                    with self.subTest(side=side, suffix=suffix, buttons=buttons):
                        cmd = self.sample({"enable": True, **buttons}, axes)
                        self.assertEqual(cmd["mode"], f"{side.title()} arm")
                        nonzero = [(n, v) for n, v in zip(names, cmd["velocities"]) if v]
                        self.assertEqual(nonzero, [(f"{side}_{suffix}", expected)])
                        gate = CommandGuard(names)
                        safe = gate.accept(cmd, cmd["input_ns"])
                        self.assertEqual([n for n, v in zip(names, safe["velocities"]) if v], [f"{side}_{suffix}"])
                        stopped = self.sample()
                        self.assertEqual(gate.accept(stopped, stopped["input_ns"])["velocities"], [0] * 17)

    def test_mode_switch_stops_motion_and_requires_releasing_enable(self):
        self.sample()
        command = self.sample({"enable": True, "mode": True})
        self.assertEqual(command["mode"], "Left arm")
        self.assertFalse(command["enabled"])
        self.assertFalse(self.sample({"enable": True, "mode": True})["enabled"])
        self.sample()
        self.assertTrue(self.sample({"enable": True})["enabled"])

    def test_dpad_drives_and_neutral_has_clear_feedback(self):
        self.sample()
        self.assertIn("move a stick", self.sample({"enable": True})["status"])
        forward = self.sample({"enable": True, "up": True})
        turn = self.sample({"enable": True, "left": True})
        self.assertGreater(np.linalg.norm(forward["velocities"]), 0)
        wheels = [
            turn["velocities"][self.names.index(f"wheel_{i}_joint")] for i in (1, 2, 3)
        ]
        np.testing.assert_allclose(wheels, [wheels[0]] * 3)
        self.assertNotEqual(wheels[0], 0)

    def test_pause_reconnect_and_stick_deadzone(self):
        self.sample()
        self.assertTrue(self.sample({"enable": True})["enabled"])
        self.assertFalse(self.sample({"enable": True}, connected=False)["enabled"])
        self.assertFalse(self.sample({"enable": True})["enabled"])
        self.sample()
        self.assertEqual(
            self.sample({"enable": True}, {"left_y": 0.05})["velocities"], [0] * 17
        )
        self.assertFalse(self.sample({"pause": True, "enable": True})["enabled"])

    def test_guard_rejects_invalid_order_nan_and_excess(self):
        cmd = self.sample()
        gate = CommandGuard(self.names)
        gate.accept(cmd, cmd["input_ns"])
        with self.assertRaises(ValueError):
            gate.accept(cmd, cmd["input_ns"])
        for value in (float("nan"), 100):
            bad = dict(cmd, velocities=[value] * 17)
            with self.assertRaises(ValueError):
                CommandGuard(self.names).accept(bad, bad["input_ns"])

    def test_release_and_timeout_stop_without_acceleration_tail(self):
        self.sample()
        cmd = self.sample({"enable": True}, {"left_y": -1})
        gate = CommandGuard(self.names)
        safe = gate.accept(cmd, cmd["input_ns"])
        self.assertGreater(np.linalg.norm(active_velocity(safe, cmd["input_ns"])), 0)
        self.assertEqual(
            active_velocity(safe, cmd["input_ns"] + TIMEOUT_NS + 1).tolist(), [0] * 17
        )
        neutral = self.sample()
        self.assertEqual(
            gate.accept(neutral, neutral["input_ns"])["velocities"], [0] * 17
        )

    def test_limits_clamp_arms_without_clamping_continuous_wheels(self):
        target = np.zeros(17)
        index = self.names.index("left_j6_gripper")
        target[index] = self.limits[index, 1] - 0.001
        velocity = np.zeros(17)
        velocity[index] = 0.5
        velocity[0] = 5
        after = integrate_targets(target, velocity, self.limits)
        self.assertEqual(after[index], self.limits[index, 1])
        self.assertAlmostEqual(after[0], 0.05)

    def test_arm_mode_leaves_base_and_other_arm_stationary(self):
        self.sample()
        self.sample({"mode": True})
        self.sample()
        cmd = self.sample(
            {"enable": True},
            {name: 1 for name in ("left_x", "left_y", "right_x", "right_y")},
        )
        for name, value in zip(self.names, cmd["velocities"]):
            if not name.startswith("left_"):
                self.assertEqual(value, 0)
        self.assertEqual(sum(v != 0 for v in cmd["velocities"]), 4)


if __name__ == "__main__":
    unittest.main()
