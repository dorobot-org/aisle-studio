import unittest

from scene_composition import resolve_references


class SceneCompositionTests(unittest.TestCase):
    def setUp(self):
        self.sheet = dict(
            instruction="Shift the chair and turn it",
            instances=[
                dict(id="left", name="Left chair", asset_name="Chair"),
                dict(id="right", name="Right chair", asset_name="Chair"),
            ],
            assets=[dict(id="asset-chair", name="Chair")],
        )
        self.ref = dict(phrase="the chair", kind="instance", candidates=["left"])
        self.op = dict(
            action="translate",
            subject=0,
            anchor=-1,
            xyz_m=[0, 0.2, 0],
            yaw_deg=0,
            surface="",
        )
        self.plan = dict(supported=True, references=[self.ref], operations=[self.op])

    def test_a_generic_name_includes_the_other_chair_even_if_the_model_picked_one(self):
        resolve_references(self.plan, self.sheet)
        self.assertEqual(self.ref["candidates"], ["left", "right"])

    def test_an_explicit_object_name_stays_unique(self):
        self.sheet["instruction"] = "Shift the left chair"
        self.ref["phrase"] = "left chair"
        resolve_references(self.plan, self.sheet)
        self.assertEqual(self.ref["candidates"], ["left"])

    def test_invalid_edits_are_rejected_as_a_whole(self):
        for bad in [
            dict(subject=1),
            dict(action="snap", anchor=7),
            dict(action="add"),
            dict(action="teleport"),
            dict(xyz_m=[0, float("nan"), 0]),
            dict(yaw_deg=True),
        ]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                resolve_references(
                    self.plan | {"operations": [self.op, self.op | bad]}, self.sheet
                )

    def test_unrecognized_objects_and_invented_phrases_are_rejected(self):
        for bad in [
            dict(phrase="left chair"),
            dict(candidates=["plant"]),
            dict(candidates=[]),
        ]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                resolve_references(
                    self.plan | {"references": [self.ref | bad]}, self.sheet
                )

    def test_refusal_has_no_executable_operations(self):
        with self.assertRaisesRegex(ValueError, "missing lamp"):
            resolve_references(dict(supported=False, reason="missing lamp"), self.sheet)
