"""Geometric acceptance for Nano's local mirrored simulation assembly.

Checks the loaded assets, not merely the generator's transformation rules.
These checks do not establish the physical robot's mounting or calibration.
"""

import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

import numpy as np

from nano_living import NAVIGATION_POSTURE

MODEL = Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf"
MIRROR = np.diag([1.0, -1.0, 1.0, 1.0])
SUFFIXES = ("j1_yaw", "j2_pitch", "j3_pitch", "j4_pitch", "j5_roll", "j6_gripper")


def vector(element, key, default="0 0 0"):
    return np.fromstring(
        element.get(key, default) if element is not None else default, sep=" "
    )


def rotation(axis, angle):
    axis = np.asarray(axis) / np.linalg.norm(axis)
    x, y, z = axis
    skew = np.array([[0, -z, y], [z, 0, -x], [-y, x, 0]])
    return np.eye(3) + np.sin(angle) * skew + (1 - np.cos(angle)) * skew @ skew


def origin(element):
    pose = np.eye(4)
    r, p, y = vector(element, "rpy")
    pose[:3, :3] = (
        rotation([0, 0, 1], y) @ rotation([0, 1, 0], p) @ rotation([1, 0, 0], r)
    )
    pose[:3, 3] = vector(element, "xyz")
    return pose


def fk(root, values):
    poses = {"base_link": np.eye(4)}
    pending = root.findall("joint")
    while pending:
        count = len(pending)
        for joint in pending[:]:
            parent = joint.find("parent").get("link")
            if parent not in poses:
                continue
            local = origin(joint.find("origin"))
            if joint.get("type") != "fixed":
                motion = np.eye(4)
                motion[:3, :3] = rotation(
                    vector(joint.find("axis"), "xyz"), values.get(joint.get("name"), 0)
                )
                local = local @ motion
            poses[joint.find("child").get("link")] = poses[parent] @ local
            pending.remove(joint)
        if len(pending) == count:
            raise ValueError("Disconnected model")
    return poses


def triangles(path):
    data = path.read_bytes()
    dtype = np.dtype(
        [("normal", "<f4", 3), ("vertices", "<f4", (3, 3)), ("attribute", "<u2")]
    )
    count = int.from_bytes(data[80:84], "little")
    assert len(data) == 84 + 50 * count
    return np.frombuffer(data, dtype=dtype, offset=84)


def symmetry_errors(root, samples=500):
    rng = np.random.default_rng(42)
    errors = dict(position_m=0.0, rotation=0.0)
    links = [
        link.get("name")[5:]
        for link in root.findall("link")
        if link.get("name").startswith("left_")
    ]
    for _ in range(samples):
        values = {}
        for suffix in SUFFIXES:
            limit = root.find(f"joint[@name='right_{suffix}']/limit")
            q = rng.uniform(float(limit.get("lower")), float(limit.get("upper")))
            for side in ("left", "right"):
                values[f"{side}_{suffix}"] = q
        poses = fk(root, values)
        for suffix in links:
            delta = poses["left_" + suffix] - MIRROR @ poses["right_" + suffix] @ MIRROR
            errors["position_m"] = max(
                errors["position_m"], float(np.max(np.abs(delta[:3, 3])))
            )
            errors["rotation"] = max(
                errors["rotation"], float(np.max(np.abs(delta[:3, :3])))
            )
    return errors


class SymmetryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = ET.parse(MODEL).getroot()

    def test_all_link_frames_mirror_at_500_joint_configurations(self):
        errors = symmetry_errors(self.root)
        self.assertLess(errors["position_m"], 1e-10, errors)
        self.assertLess(errors["rotation"], 1e-10, errors)

    def test_parked_posture_is_symmetric(self):
        for suffix in SUFFIXES:
            self.assertEqual(
                NAVIGATION_POSTURE.get("left_" + suffix, 0),
                NAVIGATION_POSTURE.get("right_" + suffix, 0),
            )
        poses = fk(self.root, NAVIGATION_POSTURE)
        for suffix in ("shoulder", "tool_mount", "wrist_cam", "finger_a", "finger_b"):
            np.testing.assert_allclose(
                poses["left_" + suffix],
                MIRROR @ poses["right_" + suffix] @ MIRROR,
                atol=1e-10,
            )

    def test_every_visual_and_collision_surface_mirrors_with_outward_winding(self):
        for right in self.root.findall("link"):
            if not right.get("name").startswith("right_"):
                continue
            left = self.root.find(f"link[@name='left_{right.get('name')[6:]}']")
            for kind in ("visual", "collision"):
                a, b = left.findall(kind), right.findall(kind)
                self.assertEqual(len(a), len(b))
                for lv, rv in zip(a, b):
                    lm, rm = lv.find("geometry/mesh"), rv.find("geometry/mesh")
                    lt = triangles(MODEL.parent / lm.get("filename"))
                    rt = triangles(MODEL.parent / rm.get("filename"))
                    np.testing.assert_allclose(
                        origin(lv.find("origin")),
                        MIRROR @ origin(rv.find("origin")) @ MIRROR,
                        atol=1e-12,
                    )
                    np.testing.assert_array_equal(
                        lt["vertices"], rt["vertices"][:, [0, 2, 1]] * [1, -1, 1]
                    )
                    np.testing.assert_array_equal(
                        lt["normal"], rt["normal"] * [1, -1, 1]
                    )
                    self.assertTrue(np.isfinite(lt["vertices"]).all())

    def test_mass_inertia_and_joint_limits_match(self):
        for suffix in SUFFIXES:
            left = self.root.find(f"joint[@name='left_{suffix}']")
            right = self.root.find(f"joint[@name='right_{suffix}']")
            self.assertEqual(left.find("limit").attrib, right.find("limit").attrib)
            self.assertAlmostEqual(
                np.linalg.norm(vector(left.find("axis"), "xyz")), 1.0, places=9
            )
        for right in self.root.findall("link"):
            if not right.get("name").startswith("right_"):
                continue
            left = self.root.find(f"link[@name='left_{right.get('name')[6:]}']")

            def tensor(link):
                inertial = link.find("inertial")
                a = inertial.find("inertia").attrib
                matrix = np.array(
                    [
                        [float(a["ixx"]), float(a["ixy"]), float(a["ixz"])],
                        [float(a["ixy"]), float(a["iyy"]), float(a["iyz"])],
                        [float(a["ixz"]), float(a["iyz"]), float(a["izz"])],
                    ]
                )
                r = origin(inertial.find("origin"))[:3, :3]
                return r @ matrix @ r.T

            self.assertEqual(
                left.find("inertial/mass").attrib, right.find("inertial/mass").attrib
            )
            np.testing.assert_allclose(
                tensor(left),
                MIRROR[:3, :3] @ tensor(right) @ MIRROR[:3, :3],
                atol=1e-12,
            )
            self.assertGreater(np.linalg.eigvalsh(tensor(left)).min(), 0)

    def test_each_joint_moves_only_its_arm_in_mirrored_direction(self):
        neutral = fk(self.root, {})
        for suffix in SUFFIXES:
            poses = fk(self.root, {"left_" + suffix: 0.2, "right_" + suffix: 0.2})
            for side in ("left", "right"):
                child = self.root.find(f"joint[@name='{side}_{suffix}']/child").get(
                    "link"
                )
                self.assertGreater(np.linalg.norm(poses[child] - neutral[child]), 0.1)
            left_only = fk(self.root, {"left_" + suffix: 0.2})
            for name in neutral:
                if name.startswith("right_"):
                    np.testing.assert_array_equal(left_only[name], neutral[name])


if __name__ == "__main__":
    unittest.main()
