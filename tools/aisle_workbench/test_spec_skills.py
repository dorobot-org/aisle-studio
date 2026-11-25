import json
import tempfile
import unittest
from pathlib import Path
from shutil import copytree
from unittest.mock import patch

from spec_catalog import BUILTIN, describe, digest, encode, listed, resolved
from spec_runtime import verify_trial


class PackageIdentityTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.install = self.root / "installed"
        copytree(BUILTIN, self.install)
        self.pin = describe(self.install, "Test", "Nano")
        self.registry = patch(
            "spec_catalog.sources", return_value={"Test": self.install}
        )
        self.registry.start()
        self.addCleanup(self.registry.stop)

    def test_entire_package_tree_and_entry_point_mode_belong_to_identity(self):
        snapshot = self.root / "snapshot"
        self.assertEqual(resolved(self.pin, "Nano", snapshot), self.pin)
        self.assertEqual(describe(snapshot, "Test", "Nano"), self.pin)
        (self.install / "helper.json").write_text('{"scale":2}')
        with self.assertRaisesRegex(ValueError, "differs"):
            resolved(self.pin, "Nano")
        (self.install / "helper.json").unlink()
        (self.install / "main").chmod(0o644)
        with self.assertRaisesRegex(ValueError, "executable main"):
            resolved(self.pin, "Nano")

    def test_one_invalid_source_keeps_the_remaining_catalog_available(self):
        bad = self.root / "invalid"
        copytree(self.install, bad)
        manifest = json.loads((bad / "manifest.json").read_text())
        manifest["aisle"]["profiles"]["Nano"]["sensors"].append("privileged_depth")
        (bad / "manifest.json").write_text(json.dumps(manifest))
        with patch(
            "spec_catalog.sources", return_value={"Test": self.install, "Invalid": bad}
        ):
            entries, errors = listed("Nano")
        self.assertEqual(entries, [self.pin])
        self.assertIn("observation contract", errors[0])

    def test_device_preflight_rechecks_the_independent_measurement(self):
        lock = dict(package=self.pin, runtime={"node_sha256": "expected"})
        plan = dict(robot="Nano", spec_skill=lock)
        data = encode(plan)
        (self.root / "controller.json").write_bytes(data)
        measurement = dict(task_success=True, physics_ticks=37)
        raw = encode(measurement)
        (self.root / "spec-measurement.json").write_bytes(raw)
        receipt = dict(
            request_id="test-request",
            controller_sha256=digest(data),
            package=self.pin,
            package_sha256=self.pin["package_sha256"],
            runtime=lock["runtime"],
            verb=self.pin["verb"],
            measured_result_sha256=digest(raw),
            task_success=True,
        )
        body = encode(receipt)
        (self.root / "spec-receipt.json").write_bytes(body)
        result = measurement | dict(
            spec_skill=lock,
            spec_request_id="test-request",
            spec_receipt_sha256=digest(body),
        )
        with patch("spec_runtime.lock_selection", return_value=lock):
            self.assertEqual(verify_trial(self.root, result, plan)["package"], self.pin)
            with self.assertRaisesRegex(ValueError, "measurement differs"):
                verify_trial(self.root, result | {"physics_ticks": 38}, plan)
            (self.root / "spec-measurement.json").write_bytes(
                encode(measurement | {"physics_ticks": 38})
            )
            with self.assertRaisesRegex(ValueError, "measurement differs"):
                verify_trial(self.root, result, plan)
