import base64
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

from authored_scene import FLOOR_Z, load
from fleet_client import submission
from fleet_worker import payload
from worker import describe

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_bound_pickup import INSTRUCTION, fixture


class AuthoredSceneTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        scene, assets = fixture(self.root / "survey")
        self.doc = dict(
            schema="aisle-studio-nano-pickup-v1",
            robot="Nano",
            instruction=INSTRUCTION,
            scene=scene,
            assets=assets,
            pickup="object-red",
            placement=dict(instance="object-oak", surface="Landing area"),
        )
        self.path = self.root / "bound-task.json"

    def compile(self):
        self.path.write_text(json.dumps(self.doc))
        return load(self.path)

    def test_room_registration_projects_bodies_into_robot_start_frame(self):
        original = self.compile()
        self.doc["scene"]["registration"] = dict(robot_xy_yaw=[-0.6, -0.5, 90], fixed_cameras={"wall": [0,0,2,0,90,0]})
        registered = self.compile()
        for before, after in zip(original.specs, registered.specs, strict=True):
            self.assertAlmostEqual(after["position"][0], before["position"][1] + 0.5)
            self.assertAlmostEqual(after["position"][1], -(before["position"][0] + 0.6))
        self.doc["scene"]["registration"]["robot_xy_yaw"][0] = 500
        with self.assertRaisesRegex(ValueError, "outside"):
            self.compile()

    def test_rotated_table_and_renamed_target_use_pinned_body_ids(self):
        self.doc["scene"]["instances"][0].update(yaw_deg=90.0, position_m=[0, 2, 0])
        self.doc["scene"]["instances"][1]["name"] = "Package seven"
        bound = self.compile()
        leg = next(b for b in bound.specs if b.get("collider") == "leg-0")
        self.assertAlmostEqual(leg["position"][0], 0.36)
        self.assertAlmostEqual(leg["position"][1], 1.34)
        top = next(b for b in bound.specs if b["name"] == bound.tabletop)
        self.assertAlmostEqual(
            top["position"][2] + top["size"][2] / 2, 0.5743 + FLOOR_Z
        )
        self.assertEqual(bound.surface["size_m"], [1.4, 0.8])
        self.assertEqual(bound.surface["appearance"], "oak")
        self.assertEqual(
            next(b for b in bound.specs if b["name"] == bound.cube)["mass_kg"], 0.032
        )

    def test_reject_unresolvable_and_obstructed_inputs(self):
        self.doc["assets"][2]["physics"]["appearance"]["value"] = "red"
        with self.assertRaisesRegex(ValueError, "target RGB appearance"):
            self.compile()
        self.doc["assets"][2]["physics"]["appearance"]["value"] = "blue"
        self.doc["scene"]["instances"][2]["position_m"] = [0, 0, 0]
        with self.assertRaisesRegex(ValueError, "start pose"):
            self.compile()
        self.doc["scene"]["instances"].pop()
        self.doc["placement"]["surface"] = "Removed"
        with self.assertRaisesRegex(ValueError, "surface is missing"):
            self.compile()

    def test_fleet_verifies_the_exact_frozen_scene_before_acceptance(self):
        bound = self.compile()
        plan = dict(
            kind="table-transfer",
            color_order="red",
            robot="Nano",
            provider="reference",
            model_hash=describe("Nano")["model_hash"],
            bound_task_sha256=bound.sha256,
        )
        controller = self.root / "controller.json"
        controller.write_text(json.dumps(plan))
        request = submission(controller)
        self.assertIn("bound-task.json", request["files"])
        payload(request)
        request["files"]["bound-task.json"] = base64.b64encode(b"{}").decode()
        with self.assertRaisesRegex(ValueError, "bound scene task changed"):
            payload(request)
