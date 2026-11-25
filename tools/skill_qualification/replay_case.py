"""Run a candidate skill on a disclosed Nano layout and verify the paired measurements."""

import argparse
import json
from pathlib import Path
import subprocess
import sys

from cohort import TOOLS, digest, encode, nano, read, save_atomic, sha, spec_runtime
from review import checked_receipt, release_packets


def disclosed_case(baseline, published, index):
    report = read(published)
    registration = read(baseline / "registration.json")
    rows = read(baseline / "qualification-rows.json")
    manifest = read(baseline / "suite.json")
    if not (
        registration["robot"] == "Nano"
        and baseline.name
        == report["registration_sha256"]
        == digest(encode(registration))
        and sha(baseline / "suite.json") == registration["manifest_sha256"]
        and read(baseline / "verification.json") == report
        and report["rows_sha256"] == digest(encode(rows))
        and manifest["split"] == "hidden"
        and report["verified"] == len(rows) == 50
        and 0 <= index < len(rows)
    ):
        raise ValueError(
            "Choose a fully disclosed, verified Nano cohort and its published verdict"
        )
    row = rows[index]
    if (
        row["index"] != index
        or nano.signature(row["opening"]) != manifest["commitments"][index]
    ):
        raise ValueError("The selected case differs from the frozen layout commitment")
    return registration, manifest, row


def replay(baseline, published, index, controller, output, inspect):
    registration, original, row = disclosed_case(baseline, published, index)
    plan = read(controller)
    skill = plan.get("spec_skill")
    if (
        plan["robot"] != "Nano"
        or plan["kind"] != "table-transfer"
        or not skill
        or any(k in plan for k in ("bound_task_sha256", "scene_observation"))
        or spec_runtime.lock_selection(skill["package"], "Nano") != skill
        or plan["color_order"].split(",") != original["color_order"]
    ):
        raise ValueError(
            "Select the installed Nano skill and the same three-cube reference task"
        )
    inputs = nano.inputs()

    # Keep the original scene, scoring code, URDF and physics engine fixed.
    # A controller developer may change the policy implementation and prepared parameters.
    def fixed(settings):
        files = settings["studio_files"].copy()
        files.pop("tools/aisle-validation/nano_rgb_transfer_policy.py")
        return settings | {"studio_files": files}

    if fixed(original["inputs"]) != fixed(inputs):
        raise ValueError(
            "The original scene, geometry, evaluator or physics runtime changed"
        )
    member = row["opening"]
    if not inspect:
        if output.exists():
            raise ValueError(
                "Choose a fresh trial directory or use --inspect to replay its measurements"
            )
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.with_suffix(".log").open("xb") as log:
            subprocess.run(
                [
                    sys.executable,
                    str(TOOLS / "aisle_workbench/worker.py"),
                    "run",
                    "--robot",
                    "Nano",
                    "--controller",
                    str(controller.resolve()),
                    "--out",
                    str(output.resolve()),
                    "--seed",
                    str(member["seed"]),
                    "--observations-only",
                    "--observation-period-s",
                    "5",
                ],
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
            )
    before = row["spec"]
    after = read(output / "result.json")
    candidate = dict(
        robot="Nano",
        controller_sha256=sha(controller),
        spec_skill=skill,
        observation_inputs=spec_runtime.observation_inputs(plan),
    )
    receipt = checked_receipt(output, candidate, plan, after)
    if (
        receipt["request_id"] == before["request_id"]
        or receipt["run_id"] == before["run_id"]
    ):
        raise ValueError("The candidate must have its own measured SPEC execution")
    scoring = original | {
        "inputs": inputs,
        "controller_sha256": candidate["controller_sha256"],
    }
    measured = nano.receipt(output, member, scoring)
    if not measured.get("verified") or measured["passed"] != after["task_success"]:
        raise ValueError("Candidate physics replay disagrees with the returned verdict")
    packets = release_packets(output)
    return dict(
        split="disclosed development",
        original_registration=baseline.name,
        baseline_controller_sha256=registration["controller_sha256"],
        candidate_controller_sha256=candidate["controller_sha256"],
        case=index,
        baseline_passed=row["passed"],
        candidate_passed=measured["passed"],
        physics_ticks=measured["physics_ticks"],
        bypasses=measured["guard_bypasses"],
        error=measured["error"],
        release_exposures=len(packets),
        evidence=measured
        | {"artifacts": {"release_packets_sha256": digest(encode(packets))}},
        baseline_receipt_sha256=before["receipt_sha256"],
        candidate_receipt=receipt,
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for field in ("baseline", "published-verdict", "controller", "out"):
        parser.add_argument("--" + field, required=True, type=Path)
    parser.add_argument("--case", required=True, type=int)
    parser.add_argument(
        "--inspect",
        action="store_true",
        help="Verify an existing measured candidate trial in the output directory",
    )
    parser.add_argument(
        "--expected-verdict", help="Digest of the verdict the operator inspected"
    )
    parser.add_argument(
        "--remember",
        action="store_true",
        help="Save a reference to this development trial for the project",
    )
    args = parser.parse_args()
    if (
        args.expected_verdict
        and digest(encode(read(args.published_verdict))) != args.expected_verdict
    ):
        parser.error("The published verdict differs from the reviewed selection")
    if args.remember and args.inspect:
        parser.error("Use --remember when launching a new development trial")
    result = replay(
        args.baseline,
        args.published_verdict,
        args.case,
        args.controller,
        args.out,
        args.inspect,
    )
    if args.remember:
        result["trial"] = args.out.name
        save_atomic(args.out / "comparison.json", result)
        save_atomic(args.out.parent / "latest.json", result)
    print(json.dumps(result, indent=2))
