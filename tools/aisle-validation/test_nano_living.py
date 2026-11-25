import unittest
import numpy as np
from nano_living import (
    Navigator,
    WheelGuard,
    room_specs,
    obstacles,
    plan_path,
    clear_segment,
    wheel_rates,
)


class LivingTests(unittest.TestCase):
    def test_continuous_navigation_reverses_route_without_finishing(self):
        goals = [[1.25, 0.0], [2.2, 1.6]]
        navigator = Navigator(room_specs(), goals, continuous=True)
        navigator.command([*goals[0], 0], [1, 0, 0, 0])
        back = navigator.command([*goals[1], 0], [1, 0, 0, 0])
        self.assertFalse(navigator.completed)
        self.assertEqual((navigator.legs, navigator.goal_index), (1, 0))
        self.assertGreater(float(np.linalg.norm(back)), 0)
        forward = navigator.command([*goals[0], 0], [1, 0, 0, 0])
        self.assertEqual((navigator.legs, navigator.goal_index), (2, 1))
        self.assertGreater(float(np.linalg.norm(forward)), 0)

    def test_path_goes_around_furniture_with_full_footprint(self):
        boxes = obstacles(room_specs())
        start = [2, 0]
        goal = [-2, 3.9]
        path = plan_path(start, goal, boxes)
        self.assertGreater(len(path), 1)
        for a, b in zip([start] + path, path):
            self.assertTrue(clear_segment(a, b, boxes))
        self.assertEqual(path[-1], goal)

    def test_rejects_goal_inside_sofa(self):
        with self.assertRaises(ValueError):
            plan_path([2, 0], [0, 2.6], obstacles(room_specs()))

    def test_guard_rejects_nan_limit_and_reordering(self):
        for rates, seq in [([float("nan"), 0, 0], 0), ([6, 0, 0], 0), ([0, 0, 0], 1)]:
            with self.assertRaises(ValueError):
                WheelGuard().accept(rates, seq)
        g = WheelGuard()
        self.assertEqual(g.accept([5, -5, 0], 0).tolist(), [1, -1, 0])

    def test_controller_is_feedback_based_and_respects_wheel_limit(self):
        n = Navigator(room_specs(), [[1.25, 0]])
        a = n.command([0, 0, 0], [1, 0, 0, 0])
        b = n.command([1.25, 0, 0], [1, 0, 0, 0])
        self.assertGreater(float(np.linalg.norm(a)), 0)
        self.assertEqual(b.tolist(), [0, 0, 0])
        self.assertTrue(n.completed)
        self.assertLessEqual(float(np.abs(wheel_rates(3, 4, 2)).max()), 5.0 + 1e-9)

    def test_seed_reproduces_scene(self):
        self.assertEqual(room_specs(4), room_specs(4))
        self.assertNotEqual(room_specs(4), room_specs(5))

    def test_controller_turns_without_forward_drive_when_heading_is_wrong(self):
        navigator = Navigator(room_specs(), [[1.25, 0.0]])
        rates = navigator.command([0, 0, 0], [np.cos(0.2), 0, 0, np.sin(0.2)])
        # Equal wheel rates are pure rotation in the calibrated model.
        self.assertGreater(float(np.linalg.norm(rates)), 0.0)
        np.testing.assert_allclose(rates, np.full(3, rates[0]), atol=1e-9)


class StreamTests(unittest.TestCase):
    def test_preview_only_keeps_latest_state_without_creating_recordings(self):
        import json
        import tempfile
        from pathlib import Path
        from nano_living_record import PreviewProgress

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            writer = PreviewProgress({"output": directory}, {"fps": 5})
            data = {"observation": np.zeros(41), "contacts": np.zeros((0, 2))}
            for seq in range(3):
                writer.append(
                    data,
                    {
                        "sequence": seq,
                        "sim_time_ns": (seq + 1) * 200_000_000,
                        "phase": "preview",
                    },
                    [0] * 3,
                    [0] * 3,
                )
            progress = json.loads((root / "progress.json").read_text())
            self.assertEqual(progress["preview_frames"], 3)
            self.assertFalse(progress["recording"])
            self.assertEqual(
                {p.name for p in root.iterdir()}, {"progress.json", "scene-info.json"}
            )
            with self.assertRaises(ValueError):
                writer.append(
                    data,
                    {"sequence": 5, "sim_time_ns": 1_200_000_000},
                    [0] * 3,
                    [0] * 3,
                )
            result = writer.finish(False)
            self.assertIsNone(result["dataset"])
            self.assertIsNone(result["raw"])

    def test_final_producer_waits_for_matching_recording_ack(self):
        from nano_living_node import wait_recorded

        class Node:
            def __init__(self, events):
                self.events = iter(events)
                self.read = 0

            def next(self, timeout):
                self.read += 1
                return next(self.events)

        def ack(sequence):
            return {"type": "INPUT", "id": "ack", "metadata": {"sequence": sequence}}

        node = Node([ack(16), ack(17), ack(18)])
        wait_recorded(node, 18)
        self.assertEqual(node.read, 3)
        with self.assertRaisesRegex(RuntimeError, "Dora stopped unexpectedly"):
            wait_recorded(Node([ack(17), {"type": "STOP"}]), 18)

    def test_navigation_posture_respects_urdf_and_raises_wrist_views(self):
        import xml.etree.ElementTree as ET
        from pathlib import Path
        from nano_living import NAVIGATION_POSTURE
        from verify_artifacts import link_poses

        urdf = ET.parse(
            Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf"
        ).getroot()
        joints = [j for j in urdf.findall("joint") if j.get("type") != "fixed"]
        names = [j.get("name") for j in joints]
        for joint in joints:
            if joint.get("name") in NAVIGATION_POSTURE:
                limit = joint.find("limit")
                self.assertTrue(
                    float(limit.get("lower"))
                    <= NAVIGATION_POSTURE[joint.get("name")]
                    <= float(limit.get("upper"))
                )
        poses = link_poses(urdf, names, [NAVIGATION_POSTURE.get(n, 0.0) for n in names])
        for link in ["left_wrist_cam", "right_wrist_cam"]:
            forward = poses[link][:3, :3] @ (np.array([1, 0, 1]) / np.sqrt(2))
            self.assertGreater(forward[2], -0.3)

    def test_visual_materials_resolve_named_and_inline_colors(self):
        import tempfile
        from pathlib import Path
        from nano_visuals import visual_specs

        with tempfile.TemporaryDirectory() as folder:
            model = Path(folder) / "robot.urdf"
            model.write_text(
                '<robot><material name="orange"><color rgba="1 .3 .035 1"/></material><link name="arm"><visual><geometry><mesh filename="arm.stl"/></geometry><material name="orange"/></visual></link><link name="cover"><visual><geometry><mesh filename="cover.stl"/></geometry><material name="orange"><color rgba=".018 .020 .024 1"/></material></visual></link></robot>'
            )
            specs = visual_specs(model)
            self.assertEqual(specs[0]["rgba"], [1, 0.3, 0.035, 1])
            self.assertEqual(specs[1]["rgba"], [0.018, 0.020, 0.024, 1])

    def test_archive_is_lossless_and_can_be_read_as_npz(self):
        import tempfile
        from pathlib import Path
        from nano_living_record import save_source

        values = np.arange(96, dtype=np.float32).reshape(8, 12)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sample.npz"
            save_source(path, depth=values)
            with np.load(path, allow_pickle=False) as source:
                np.testing.assert_array_equal(source["depth"], values)

    def test_slow_preview_client_cannot_block_publication(self):
        import socket
        import tempfile
        import time
        from pathlib import Path
        from nano_living_stream import PreviewHub

        with tempfile.TemporaryDirectory(prefix="nano-", dir="/tmp") as directory:
            hub = PreviewHub(Path(directory) / "live.sock")
            client = socket.socket(socket.AF_UNIX)
            client.connect(str(hub.path))
            start = time.monotonic()
            try:
                for i in range(20):
                    hub.publish({"sequence": i}, [bytes(100_000)])
                self.assertLess(time.monotonic() - start, 1.0)
                self.assertEqual(hub.generation, 20)
            finally:
                client.close()
                hub.close()


if __name__ == "__main__":
    unittest.main()
