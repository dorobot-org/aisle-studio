import json
from pathlib import Path
import sys
import tempfile
import unittest

from experiment_job import create, read, write
from trial_revision import annotations, baseline, rerun, save_variant
from worker import describe, digest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_experiment import experiment_fixture


class TrialRevisionTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.task, cfg = experiment_fixture(self.root / "survey")
        self.plan = dict(
            kind="table-transfer",
            robot="Nano",
            model_hash=describe("Nano")["model_hash"],
            color_order="red",
            provider="reference",
        )
        self.controller = self.freeze(self.root / "initial", self.task)
        cfg.update(repeat_count=1, overrides={})
        self.batch = create(self.root / "experiments", cfg, self.controller)
        job = read(self.batch / "batch.json")
        row = job["repetitions"][0]
        self.id = row["run_id"]
        trial = self.batch / "trials" / self.id
        write(trial / "result.json", {"task_success": False})
        row.update(
            status="failed",
            outcome={"passed": False, "result_sha256": digest(trial / "result.json")},
        )
        write(trial / "manifest.json", row)
        write(self.batch / "batch.json", job)
        self.raw = digest(trial / "manifest.json")

    def freeze(self, folder, task):
        folder.mkdir()
        write(folder / "bound-task.json", task)
        write(
            folder / "controller.json",
            self.plan | {"bound_task_sha256": digest(folder / "bound-task.json")},
        )
        return folder / "controller.json"

    def test_edit_conflicts_and_reproducing_original(self):
        edit = dict(
            name="Review",
            notes="Unstable",
            tags=["failed"],
            fields={"score": 2},
            revision=0,
        )
        self.assertEqual(annotations(self.batch, self.id, edit)["revision"], 1)
        with self.assertRaisesRegex(ValueError, "another window"):
            annotations(self.batch, self.id, edit)
        annotations(self.batch, self.id, edit | {"revision": 1, "notes": "Rechecked"})
        self.assertEqual(
            len(list((self.batch / "annotations" / self.id).glob("*.json"))), 2
        )
        result = read(rerun(self.batch, self.id) / "batch.json")
        self.assertEqual(result["parent"]["run_id"], self.id)
        row = result["repetitions"][0]
        self.assertNotEqual(row["run_id"], self.id)
        self.assertEqual(row["sample_sha256"], result["parent"]["sample_sha256"])
        self.assertEqual(
            digest(self.batch / "trials" / self.id / "manifest.json"), self.raw
        )

    def test_variant_pins_revised_task_controller_features_seeds_and_channels(self):
        self.task["scene"]["revision"] = 2
        self.task["scene"]["instances"][-1]["position_m"][1] = -2.05
        revised = self.freeze(self.root / "revised", self.task)
        data = dict(
            name="Two seeds",
            seeds=[12, 45],
            pose_edits={"chair-left": {"y": -2.15}},
            recording={"robot_trace": True, "agent_history": False},
            record_cameras=False,
            parameters=[],
            prepared_controller=str(revised),
            prepared_sha256=digest(revised),
        )
        v = save_variant(self.batch, self.id, data)
        revised.unlink()
        child = rerun(self.batch, self.id, v["id"])
        j = read(child / "batch.json")
        self.assertEqual(j["configuration"]["scene"]["revision"], 2)
        self.assertFalse(j["configuration"]["record_cameras"])
        self.assertEqual([r["seed"] for r in j["repetitions"]], [12, 45])
        self.assertEqual(
            [r["sample_sha256"] for r in j["repetitions"]], v["samples_sha256"]
        )
        for row in j["repetitions"]:
            self.assertEqual(row["recording"], data["recording"])
            self.assertEqual(row["controller_sha256"], v["controller_sha256"])
            self.assertEqual(row["parent_run_id"], self.id)
            layout = read(child / "trials" / row["run_id"] / "sampled-layout.json")
            measured = {i["id"]: i["position_m"] for i in layout["scene"]["instances"]}
            self.assertEqual(measured["chair-left"][1], -2.15)
            self.assertEqual(measured["chair-right"][1], -2.05)
        self.assertEqual(
            digest(self.batch / "trials" / self.id / "manifest.json"), self.raw
        )

    def test_rejects_changed_parent_evidence_and_unbound_edits(self):
        with self.assertRaisesRegex(ValueError, "Missing bound instance"):
            save_variant(
                self.batch,
                self.id,
                dict(name="Unbound", pose_edits={"absent": {"x": 0}}),
            )
        write(self.batch / "trials" / self.id / "result.json", {"task_success": True})
        with self.assertRaisesRegex(ValueError, "checksum changed"):
            baseline(self.batch, self.id)
