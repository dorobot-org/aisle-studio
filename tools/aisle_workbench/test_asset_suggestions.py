"""A survey ruler calibrates one axis only when explicitly tied to the source pixels."""

from copy import deepcopy
import unittest
import numpy as np

from asset_suggestions import planar_scale


class PlanarScaleTests(unittest.TestCase):
    def setUp(self):
        ends = [[10, 20], [70, 100]]
        self.sheet = dict(
            marks=dict(rulers=[[], ends, []]),
            flat_reference=dict(axis=1, length_m=0.20, endpoints=deepcopy(ends)),
        )

    def test_independent_scale_follows_reference_direction(self):
        axis, direction, m_per_px = planar_scale(self.sheet, (200, 200))
        self.assertEqual(axis, "depth")
        np.testing.assert_allclose(direction, [0.6, 0.8])
        self.assertAlmostEqual(m_per_px, 0.002)
        del self.sheet["flat_reference"]
        self.assertIsNone(planar_scale(self.sheet, (200, 200)))

    def test_stale_degenerate_or_invalid_reference_fails(self):
        for key, value in [
            ("axis", True),
            ("axis", 3),
            ("length_m", 0),
            ("length_m", float("inf")),
            ("endpoints", [[10, 20], [70, 99]]),
        ]:
            sheet = deepcopy(self.sheet)
            sheet["flat_reference"][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                planar_scale(sheet, (200, 200))
        for ends in [[[0, 0], [0, 0]], [[0, 0], [0, 1]], [[0, 0], [200, 20]]]:
            self.sheet["marks"]["rulers"][1] = self.sheet["flat_reference"][
                "endpoints"
            ] = ends
            with self.subTest(ends=ends), self.assertRaises(ValueError):
                planar_scale(self.sheet, (200, 200))
