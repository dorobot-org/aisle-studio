import unittest
from argparse import Namespace
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from zipfile import ZipFile

import numpy as np

from g1_reach_suite import safe_npz
from g1_stand import impact_parameters, launch_position
from worker import ROOT, describe, prepare


class G1ReachTests(unittest.TestCase):
    def test_revision_rejects_evidence_from_another_scene_task(self):
        archive = ROOT / "docs/product/aisle-studio/environment/g1-verification/matched-upright/matched.zip"
        with TemporaryDirectory() as tmp, ZipFile(archive) as z:
            prior = Path(tmp) / "previous"
            out = Path(tmp) / "revision"
            prior.mkdir()
            out.mkdir()
            for name in ("bound-task.json", "result.json", "controller.json"):
                (prior / name).write_bytes(z.read("case-014/" + name))
            result = json.loads((prior / "result.json").read_bytes())
            result["bound_task_sha256"] = "0" * 64
            (prior / "result.json").write_text(json.dumps(result))
            args = Namespace(robot="G1", bound_task=prior/"bound-task.json",
                out=out, feedback=prior, provider="codex", task="Preserve the shoulder reach")
            with self.assertRaisesRegex(ValueError, "from this saved G1 task"):
                prepare(args, describe("G1"))
            self.assertFalse((out / "controller-request.json").exists())

    def test_torso_aim_compensates_gravity_across_speed_envelope(self):
        for speed in (1.1, 1.2, 1.3):
            recipe = dict(mass_kg=0.25, speed_m_s=speed, lateral_m=0.011)
            x, lateral, z = launch_position(recipe, "torso-v2")
            self.assertEqual(lateral, recipe["lateral_m"])
            self.assertAlmostEqual(z - 0.5 * 9.81 * ((x - 0.05) / speed) ** 2, 0.195)
            self.assertEqual(launch_position(recipe, "pelvis-v1"), [0.29, 0.011, 0.16])

    def test_frontal_aim_clears_the_robot_and_compensates_drop_to_the_contact_plane(self):
        for speed in (1.1, 1.2, 1.3):
            recipe = dict(mass_kg=0.25, speed_m_s=speed, lateral_m=-0.015)
            x, y, z = launch_position(recipe, "frontal-torso-v3")
            self.assertEqual(y, recipe["lateral_m"])
            self.assertAlmostEqual(x - 0.14, 0.055)
            self.assertAlmostEqual(z - 0.5 * 9.81 * ((x - 0.14) / speed) ** 2, 0.195)
        with self.assertRaises(ValueError):
            launch_position(recipe, "unknown")

    def test_closed_envelope(self):
        recipe = dict(mass_kg=0.2, speed_m_s=1.3, lateral_m=-0.015)
        self.assertEqual(impact_parameters(recipe), recipe)
        for name, value in (
            ("mass_kg", 0.31),
            ("speed_m_s", 1.31),
            ("lateral_m", -0.016),
            ("mass_kg", True),
            ("speed_m_s", float("nan")),
        ):
            with self.subTest(name=name, value=value), self.assertRaises(ValueError):
                impact_parameters(recipe | {name: value})

    def test_reject_unexpected_shape_before_loading_a_trace(self):
        from io import BytesIO

        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "wrist.npz"
            stream = BytesIO()
            np.lib.format.write_array_header_1_0(
                stream, dict(descr="<f8", fortran_order=False, shape=(2**36, 3))
            )
            with ZipFile(path, "x") as z:
                z.writestr("measured_m.npy", stream.getvalue())
            with self.assertRaisesRegex(ValueError, "array header"):
                safe_npz(path, {"measured_m": (2000, 3)})
