"""Replay registered Nexus measurements and bind a reviewer's signature to the verdict."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
from tempfile import NamedTemporaryFile

import cohort
from cohort import digest, encode, read, save_atomic, sha
from nano_suite_evidence import verify_release_packet

NAMESPACE = "aisle-studio-simulation-qualification-v1"


def checked_receipt(case, reg, plan, result):
    verified = cohort.spec_runtime.verify_trial(case, result, plan)
    record = read(case / "spec-receipt.json")
    if (
        verified["package"] != reg["spec_skill"]["package"]
        or sha(case / "controller.json") != reg["controller_sha256"]
        or record["observation_inputs"] != reg["observation_inputs"]
        or record["backend"] != "Nexus"
        or not record["request_id"]
        or not record["run_id"]
    ):
        raise ValueError(
            "Trial used a different controller, sensor contract or backend"
        )
    return {
        "request_id": record["request_id"],
        "run_id": record["run_id"],
        "receipt_sha256": verified["receipt_sha256"],
        "measurement_sha256": record["measured_result_sha256"],
    }


def release_packets(case):
    packets = {}
    calibration = read(case / "calibration.json")
    with (case / "actions.jsonl").open() as stream:
        for line in stream:
            action = json.loads(line)
            if action.get("op") == "release observation":
                name = f"{action['tick']:06}.npz"
                raw = (case / "release-observations" / name).read_bytes()
                verify_release_packet(raw, calibration, action)
                packets[name] = digest(raw)
    if set(packets) != {p.name for p in (case / "release-observations").glob("*.npz")}:
        raise ValueError("Release exposures differ from the recorded actions")
    return packets


def verify(directory):
    reg, manifest, openings = cohort.frozen(directory)
    plan = read(directory / "controller.json")
    rows, requests, runs = [], set(), set()
    for index, opening in enumerate(openings):
        case = directory / f"case-{index:03}"
        attempt = directory / f"attempt-{index:03}.json"
        row = {"index": index, "opening": opening, "verified": False, "passed": False}
        try:
            if not (
                case.with_suffix(".log").is_file()
                and attempt.is_file()
                and read(attempt)["exit_code"] == 0
            ):
                raise ValueError("Attempt has no successful worker exit")
            result = read(case / "result.json")
            receipt = checked_receipt(case, reg, plan, result)
            if receipt["request_id"] in requests or receipt["run_id"] in runs:
                raise ValueError("Reused SPEC request or run identity")
            requests.add(receipt["request_id"])
            runs.add(receipt["run_id"])
            if reg["robot"] == "Nano":
                scored = cohort.nano.receipt(case, opening, manifest)
                packets = release_packets(case)
                files = {
                    "release_packets_sha256": digest(encode(packets)),
                    "release_exposures": len(packets),
                }
            else:
                scored = cohort.g1.score(case, manifest, opening)
                files = {
                    name: sha(case / name)
                    for name in cohort.g1.FILES
                    if (case / name).is_file()
                }
            if scored["passed"] != result["task_success"]:
                raise ValueError("Evaluator and recorded task verdict differ")
            row.update(scored, spec=receipt, artifacts=files)
        except (ValueError, KeyError, TypeError, OSError, EOFError) as exc:
            row["issue"] = str(exc).replace(str(directory), ".")
        rows.append(row)
    finished = len(list(directory.glob("case-*.log")))
    if finished != reg["cases"]:
        raise ValueError(
            f"{finished}/{reg['cases']} slots consumed. Run the remaining cases before review."
        )
    successes = sum(r["passed"] for r in rows)
    verified = sum(r["verified"] for r in rows)
    bypasses = sum(r.get("guard_bypasses", 0) for r in rows)
    report = dict(
        version=1,
        scope="Nexus simulation",
        registration_sha256=directory.name,
        robot=reg["robot"],
        controller_sha256=reg["controller_sha256"],
        package_sha256=reg["spec_skill"]["package"]["package_sha256"],
        verifier_sha256=sha(Path(__file__)),
        cases=reg["cases"],
        required=reg["required"],
        successes=successes,
        verified=verified,
        guard_bypasses=bypasses,
        qualified=verified == reg["cases"]
        and successes >= reg["required"]
        and bypasses == 0,
        rows_sha256=digest(encode(rows)),
        failed_cases=[r["index"] for r in rows if not r["passed"]],
    )
    if (directory / "reviews.json").is_file():
        history(directory, report)
    save_atomic(directory / "qualification-rows.json", rows)
    save_atomic(directory / "verification.json", report)
    return report


def principal(name):
    if not re.fullmatch(r"[A-Za-z0-9][a-zA-Z0-9.@_+-]{0,127}", name or ""):
        raise ValueError(
            "Enter the reviewer identity listed in the allowed signers file"
        )
    return name


def ssh(*args, data):
    proc = subprocess.run(
        ["ssh-keygen", *args],
        input=data,
        capture_output=True,
        timeout=20,
        start_new_session=True,
        env=os.environ | {"SSH_ASKPASS_REQUIRE": "never"},
    )
    if proc.returncode:
        raise ValueError(
            "Reviewer signature failed. Check the identity, trusted signers and configured signing key."
        )
    return proc.stdout


def authenticate(entry):
    decision = entry["decision"]
    trust = os.environ.get("DOROBOT_QUALIFICATION_ALLOWED_SIGNERS")
    if not trust:
        raise ValueError(
            "Configure DOROBOT_QUALIFICATION_ALLOWED_SIGNERS to verify reviewer signatures"
        )
    with NamedTemporaryFile() as sig:
        sig.write(entry["signature"].encode())
        sig.flush()
        ssh(
            "-Y",
            "verify",
            "-n",
            NAMESPACE,
            "-f",
            trust,
            "-I",
            principal(decision["reviewer"]),
            "-s",
            sig.name,
            data=encode(decision),
        )


def history(directory, report):
    previous = None
    entries = []
    report_sha = digest(encode(report))
    for entry in (
        read(directory / "reviews.json")
        if (directory / "reviews.json").exists()
        else []
    ):
        authenticate(entry)
        decision = entry["decision"]
        if (
            decision["registration_sha256"] != directory.name
            or decision["previous"] != previous
            or decision["sequence"] != len(entries) + 1
            or decision["verdict_sha256"] != report_sha
        ):
            raise ValueError(
                "Reviewer decision or evidence differs from the signed history"
            )
        previous = digest(encode(entry))
        entries.append(entry)
    return entries


def sign(directory, report, reviewer, disposition, rationale, expected):
    if digest(encode(report)) != expected:
        raise ValueError(
            "Evidence changed. Verify and inspect the new verdict before signing."
        )
    if disposition == "approve" and not report["qualified"]:
        raise ValueError(
            "Simulation approval requires the full measured qualification gate"
        )
    if (
        disposition not in ("approve", "reject")
        or not 1 <= len(rationale.strip()) <= 2000
    ):
        raise ValueError("Choose approve or reject and enter a review rationale")
    key = os.environ.get("DOROBOT_QUALIFICATION_SIGNING_KEY")
    if not key:
        raise ValueError("Configure DOROBOT_QUALIFICATION_SIGNING_KEY for the reviewer")
    entries = history(directory, report)
    decision = dict(
        scope="Nexus simulation",
        registration_sha256=directory.name,
        verdict_sha256=expected,
        reviewer=principal(reviewer),
        disposition=disposition,
        rationale=rationale.strip(),
        sequence=len(entries) + 1,
        previous=digest(encode(entries[-1])) if entries else None,
        at=datetime.now(timezone.utc).isoformat(),
    )
    signature = ssh(
        "-Y", "sign", "-n", NAMESPACE, "-f", key, "-", data=encode(decision)
    )
    entry = {"decision": decision, "signature": signature.decode()}
    authenticate(entry)
    save_atomic(directory / "reviews.json", [*entries, entry])


def handoff(directory, report, controller, expected):
    if not controller or sha(controller) != report["controller_sha256"]:
        raise ValueError(
            "Select the exact prepared controller used in this simulation review"
        )
    if expected != digest(encode(report)) or not report["qualified"]:
        raise ValueError("The selected evidence must still meet the simulation gate")
    decisions = history(directory, report)
    if not decisions or decisions[-1]["decision"]["disposition"] != "approve":
        raise ValueError(
            "A current trusted simulation approval is required for handoff"
        )
    dossier = dict(
        scope="Nexus simulation",
        registration=read(directory / "registration.json"),
        verification=report,
        decisions=decisions,
        physical_qualification=False,
    )
    target = directory / ("handoff-" + digest(encode(dossier)) + ".json")
    save_atomic(target, dossier)
    return target.name


def catalog(state, robot, identity=None):
    state.mkdir(parents=True, exist_ok=True)
    entries = []
    for p in state.iterdir():
        if not (p / "registration.json").is_file() or not re.fullmatch(
            r"[0-9a-f]{64}", p.name
        ):
            continue
        reg = read(p / "registration.json")
        if reg["robot"] == robot and digest(encode(reg)) == p.name:
            entries.append((p.stat().st_mtime_ns, p.name))
    ids = [name for _, name in sorted(entries, reverse=True)]
    if identity and identity not in ids:
        raise ValueError(
            "The selected qualification belongs to a different profile or its registration changed"
        )
    identity = identity or (ids[0] if ids else None)
    view = {"ids": ids, "selected": identity, "robot": robot}
    if identity:
        directory = cohort.directory_for(state, identity)
        view.update(cohort.status(directory))
        reg = read(directory / "registration.json")
        view.update(
            protocol=reg["protocol"],
            package=reg["spec_skill"]["package"],
            sensors=reg["observation_inputs"],
            controller_sha256=reg["controller_sha256"],
        )
        if view["report"]:
            report = view["report"]
            if not (directory / "qualification-rows.json").is_file():
                raise ValueError("Missing per-case verification records")
            rows = read(directory / "qualification-rows.json")
            if digest(encode(rows)) != report["rows_sha256"]:
                raise ValueError(
                    "Stored per-case records differ. Verify the measurements again."
                )
            view["failures"] = [
                {
                    "index": r["index"],
                    "reason": r.get("issue")
                    or r.get("error")
                    or "; ".join(r.get("reasons", []))
                    or "Transfer or support criteria failed",
                }
                for r in rows
                if not r["passed"]
            ]

            view["verdict_sha256"] = digest(encode(report))
            entries = history(directory, report)
            view["latest_review"] = entries[-1]["decision"] if entries else None
    return view


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "action",
        choices=["list", "register", "run", "verify", "approve", "reject", "handoff"],
    )
    parser.add_argument("--state", required=True, type=Path)
    parser.add_argument("--robot", required=True, choices=["G1", "Nano"])
    parser.add_argument("--controller", type=Path)
    parser.add_argument("--id")
    parser.add_argument("--jobs", type=int, choices=range(1, 9), default=2)
    parser.add_argument("--reviewer", default="")
    parser.add_argument("--rationale", default="")
    parser.add_argument("--expected")
    args = parser.parse_args()
    exported = None
    if args.action == "register":
        if not args.controller or read(args.controller)["robot"] != args.robot:
            raise ValueError("Select a prepared controller for this robot")
        args.id = cohort.register(args.state, args.controller)
    elif args.action != "list":
        directory = cohort.directory_for(args.state, args.id)
        if read(directory / "registration.json")["robot"] != args.robot:
            raise ValueError("Select a qualification for this robot")
        if args.action == "run":
            cohort.run(directory, args.jobs)
        with cohort.exclusive(directory / "operation.lock"):
            report = verify(directory)
            if args.action == "handoff":
                exported = handoff(directory, report, args.controller, args.expected)
            if args.action in ("approve", "reject"):
                sign(
                    directory,
                    report,
                    args.reviewer,
                    args.action,
                    args.rationale,
                    args.expected,
                )
    print(json.dumps(catalog(args.state, args.robot, args.id) | {"exported": exported}))


if __name__ == "__main__":
    main()
