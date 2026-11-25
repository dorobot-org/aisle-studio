import base64
import json
from importlib.metadata import version
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from uuid import uuid4

from experiment import encode, sample
from experiment_fleet import build_request
from experiment_job import read, write
from fleet_worker import HERE, dispatch, initialize, transaction, worker_identity
from worker import digest


class ExperimentLeaseTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        initialize(self.root, 2)
        fixture = (
            Path(__file__).resolve().parents[2]
            / "docs/product/aisle-studio/environment/experiment-verification/nano-dim-oak"
        )
        envelope = read(fixture / "candidate-request.json")
        row, cfg = envelope["manifest"], envelope["configuration"]
        with transaction(self.root) as db:
            ident = worker_identity(db)
        worker = {
            "worker_id": ident,
            "implementation_sha256": digest(HERE / "worker.py"),
            "nexus": version("dimforge-nexus3d"),
        }
        write(
            self.root / "sampled-layout.json",
            sample(read(fixture / "bound-task.json"), cfg, row["seed"]),
        )
        self.request = build_request(
            fixture / "candidate.json",
            {"worker": worker, "configuration": cfg},
            row,
            self.root,
        )

    def test_request_replay_uses_one_lease_and_detects_changed_camera_selection(self):
        with patch("fleet_worker.subprocess.Popen") as spawn:
            first = dispatch(self.root, self.request)
            self.assertEqual(dispatch(self.root, self.request)["id"], first["id"])
            self.assertEqual(
                dispatch(self.root, {"op": "status", "id": first["id"]})["worker_id"],
                self.request["worker_id"],
            )
            spawn.assert_called_once()
            files = dict(self.request["files"])
            envelope = json.loads(base64.b64decode(files["experiment.json"]))
            envelope["configuration"]["record_cameras"] = not envelope["configuration"][
                "record_cameras"
            ]
            files["experiment.json"] = base64.b64encode(encode(envelope)).decode()
            with self.assertRaisesRegex(ValueError, "different request"):
                dispatch(self.root, self.request | {"files": files})

    def test_admission_rejects_worker_runtime_and_sample_mismatch_before_lease(self):
        with patch("fleet_worker.subprocess.Popen") as spawn:
            changes = [
                ({"worker_id": str(uuid4())}, "worker has changed"),
                ({"nexus": "unavailable"}, "runtime"),
            ]
            files = dict(self.request["files"])
            envelope = json.loads(base64.b64decode(files["experiment.json"]))
            envelope["manifest"]["sample_sha256"] = "0" * 64
            files["experiment.json"] = base64.b64encode(encode(envelope)).decode()
            for change, message in [*changes, ({"files": files}, "scheduled sample")]:
                with (
                    self.subTest(message=message),
                    self.assertRaisesRegex(ValueError, message),
                ):
                    dispatch(self.root, self.request | change)
            spawn.assert_not_called()
