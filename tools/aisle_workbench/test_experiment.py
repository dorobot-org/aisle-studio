import copy
import json
from pathlib import Path
import tempfile
import unittest

from experiment import draw, recordings, sample, schedule, seeds
from experiment_recording import history
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_experiment import experiment_fixture


class ExperimentTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.task, self.config = experiment_fixture(self.root)

    def test_named_start_regions_order_conflicts_and_legacy_layouts(self):
        items = {i["id"]: i for i in self.task["scene"]["instances"]}
        floor = dict(support=None, bounds_m=[[1.8, 1.0], [4.0, 3.0]])
        surface = dict(instance="object-oak", surface="Landing area")
        region = dict(support=surface, bounds_m=[[-0.3, -0.1], [0.05, 0.2]])
        items["object-oak"]["start_area"] = floor
        items["object-oak"]["yaw_deg"] = 37
        items["object-blue"]["start_area"] = region
        baseline = copy.deepcopy(self.task)
        keys = ["object-blue", "object-oak"]

        def realize(enabled, **changes):
            return sample(
                self.task, self.config | changes | {"active_start_areas": enabled}, 42
            )

        self.assertNotIn("placements", realize([]))
        a = realize(keys)
        self.assertEqual(a, realize(keys[::-1]))
        self.assertEqual(
            a["placements"]["object-blue"]["local_xy_m"],
            realize(keys[:1])["placements"]["object-blue"]["local_xy_m"],
        )
        self.assertNotEqual(
            a["placements"]["object-blue"]["position_m"],
            realize(keys[:1])["placements"]["object-blue"]["position_m"],
        )
        self.assertEqual(self.task, baseline)
        with self.assertRaisesRegex(ValueError, "position"):
            realize(keys, pose_edits={"object-blue": {"x": 2}})
        for enabled, error in [(["missing"], "Missing"), (keys * 2, "once")]:
            with self.assertRaisesRegex(ValueError, error):
                realize(enabled)
        region["bounds_m"] = [[0, 0], [0.01, 0.02]]
        with self.assertRaisesRegex(ValueError, "footprint"):
            realize(keys)
        region["bounds_m"] = None
        region["support"] = {"instance": "object-blue", "surface": "top"}
        with self.assertRaisesRegex(ValueError, "cycle"):
            realize(keys)
        region["support"] = surface
        surface["surface"] = "missing"
        with self.assertRaisesRegex(ValueError, "named support"):
            realize(keys)

    def test_repeat_modes_and_effective_switches(self):
        self.assertEqual(seeds(self.config), [42, 43, 44, 45])
        for mode, expected in [
            ("identical", [42] * 4),
            ("explicit", [0, 9, 9, 2**32 - 1]),
        ]:
            cfg = self.config | {"seed_mode": mode, "seeds": [0, 9, 9, 2**32 - 1]}
            self.assertEqual(seeds(cfg), expected)
        self.assertEqual(
            [
                [
                    recordings(self.config, i)[key]
                    for key in ["robot_trace", "agent_history"]
                ]
                for i in range(1, 5)
            ],
            [[False, False], [True, False], [False, True], [True, True]],
        )
        with self.assertRaises(ValueError):
            seeds(self.config | {"seed_mode": "explicit", "seeds": [42]})
        first = schedule(self.config, self.task, "a", "b", "c")
        second = schedule(self.config, self.task, "a", "b", "c")
        self.assertNotEqual(first["batch_id"], second["batch_id"])
        self.assertEqual(len({r["run_id"] for r in first["repetitions"]}), 4)

    def test_realized_parameter_bindings_leave_frozen_scene_unchanged(self):
        base = copy.deepcopy(self.task)
        p = self.config["parameters"][0]
        a = sample(base, self.config, 42)
        params = [
            dict(
                name="lighting",
                type="number",
                unit="1",
                default=0.6,
                range=[0.4, 0.8],
                binding=dict(kind="ambient"),
            ),
            p,
            dict(
                name="surface friction",
                type="number",
                unit="",
                default=0.45,
                binding=dict(kind="friction", instance="object-oak"),
            ),
        ]
        b = sample(base, self.config | {"parameters": params}, 42)
        self.assertEqual(a["scene"], b["scene"])
        self.assertEqual(a["values"]["Chair spacing"], b["values"]["Chair spacing"])
        items = {i["id"]: i for i in a["scene"]["instances"]}
        self.assertAlmostEqual(
            items["chair-right"]["position_m"][0]
            - items["chair-left"]["position_m"][0],
            a["values"]["Chair spacing"]["value"],
        )
        self.assertEqual(base, self.task)
        self.assertEqual(b["materials"], {"object-oak": {"friction": 0.45}})
        self.assertEqual(
            b["environment"]["ambient_rgb"], [b["values"]["lighting"]["value"]] * 3
        )
        for bad in [
            p | {"binding": {}},
            p | {"binding": dict(kind="position", axis="x", instance="missing")},
            p | {"range": [4, 5]},
        ]:
            with self.assertRaises(ValueError):
                sample(base, self.config | {"parameters": [bad]}, 42)

    def test_history_retains_available_exchange_and_scopes_missing_stages(self):
        source = self.root / "source"
        source.mkdir()
        (source / "controller-request.json").write_text(
            json.dumps(
                dict(
                    instruction="use RGB",
                    onboard_input=dict(images=[], measurements_sha256="a" * 64),
                )
            )
        )
        (source / "reply.json").write_text(
            json.dumps(
                {"kind": "table-transfer", "authorization": "should never be included"}
            )
        )
        target = self.root / "agent-history.jsonl"
        history(
            source,
            target,
            dict(run_id="trial", controller_sha256="b" * 64),
            dict(provider="codex", policy_model="model"),
        )
        data = [json.loads(s) for s in target.read_text().splitlines()]
        self.assertEqual(
            [e["event"] for e in data],
            ["tool_result", "prompt", "response", "controller_reused"],
        )
        self.assertNotIn("should never", target.read_text())
        self.assertEqual(data[-1]["data"]["new_model_calls"], 0)
