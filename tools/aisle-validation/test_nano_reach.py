import unittest
import numpy as np

from test_nano_symmetry import MODEL, SUFFIXES, fk
from verify_arm_reach import ArmModel, MIRROR


class ReachAuditTests(unittest.TestCase):
    def test_batch_search_agrees_with_independent_scalar_fk(self):
        model = ArmModel(MODEL)
        rng = np.random.default_rng(7)
        q = rng.uniform(-1.0, 1.0, (20, 6))
        q[:, 5] = 0.3
        for side in ("left", "right"):
            batch = model.poses(side, q)
            for index, row in enumerate(q):
                expected = fk(
                    model.root, {side + "_" + n: v for n, v in zip(SUFFIXES, row)}
                )
                for name, values in batch.items():
                    np.testing.assert_allclose(
                        values[index], expected[name], atol=1e-12
                    )

    def test_search_detects_the_previous_yaw_only_mount_error(self):
        model = ArmModel(MODEL)
        result, _, left, right, _ = model.workspace(1024, 1)
        self.assertLess(result["max_mirror_error_m"], 1e-12)
        np.testing.assert_allclose(left, right * MIRROR, atol=1e-12)
        mount = model.root.find("joint[@name='left_shoulder_fixed']/origin")
        mount.set("rpy", "3.1415927 0 3.06853455358979")
        broken, *_ = model.workspace(1024, 1)
        self.assertGreater(broken["max_mirror_error_m"], 0.1)


if __name__ == "__main__":
    unittest.main()
