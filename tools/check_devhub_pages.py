"""Build the DevHub lease's checkout and exercise the standalone native Studio pages."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def build_studio():
    command = [
        os.environ.get("CARGO", "cargo"),
        "build",
        "--locked",
        "--bin",
        "aisle-studio",
        "--message-format=json-render-diagnostics",
    ]
    binary = None
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True) as proc:
        for line in proc.stdout:
            record = json.loads(line)
            if record.get("target", {}).get("name") == "aisle-studio" and record.get(
                "executable"
            ):
                binary = Path(record["executable"])
        if proc.wait():
            raise subprocess.CalledProcessError(proc.returncode, command)
    if binary is None:
        raise RuntimeError("Cargo did not report an aisle-studio executable")
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--remote", required=True, type=Path, help="Installed makepad-remote executable"
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="Fresh evidence directory on the assigned node",
    )
    parser.add_argument(
        "--provider", choices=["reference", "octos", "codex"], default="reference"
    )
    focused = parser.add_mutually_exclusive_group()
    focused.add_argument(
        "--suite",
        choices=[
            "pages",
            "photo-reviews",
            "g1-revision",
            "scene-ranges",
            "scene-agents",
            "spec-skills",
            "skill-qualification",
            "experiment-worker",
        ],
        default="pages",
        help="Choose all pages or a focused native workflow",
    )
    focused.add_argument(
        "--photo-reviews",
        action="store_const",
        const="photo-reviews",
        dest="suite",
        help="Run the survey and both authenticated proposal workflows as a focused lease",
    )
    focused.add_argument(
        "--g1-revision",
        action="store_const",
        const="g1-revision",
        dest="suite",
        help="Revise a failed authored-room G1 run using the selected authenticated provider",
    )
    parser.add_argument(
        "--worker", help="Registered worker for the experiment-worker suite"
    )
    parser.add_argument(
        "--incompatible-worker", help="Earlier worker used for the admission check"
    )
    parser.add_argument("--qualification-robot", choices=["G1", "Nano"], help="Limit the skill qualification lease to one profile")
    args = parser.parse_args()
    if args.qualification_robot and args.suite != "skill-qualification":
        parser.error("--qualification-robot requires --suite skill-qualification")
    if args.suite == "experiment-worker" and not (
        args.worker and args.incompatible_worker
    ):
        parser.error("The experiment-worker suite needs both worker aliases")
    if args.suite == "g1-revision" and args.provider == "reference":
        parser.error("--g1-revision requires --provider octos or codex")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    head = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    studio = out / "aisle-studio"
    shutil.copy2(build_studio(), studio)
    flags = [
        "--studio",
        studio,
        "--remote",
        args.remote.resolve(),
        "--backend",
        "headless",
    ]
    tools = ROOT / "tools"
    report = dict(
        studio_commit=head,
        studio_binary_sha256=hashlib.sha256(studio.read_bytes()).hexdigest(),
        backend="headless",
        checks={},
    )

    def run(name, script, *extra, receipt="verification.json", python=sys.executable):
        started = time.monotonic()
        subprocess.run(
            [python, tools / script, *flags, "--session", out / name, *extra],
            cwd=ROOT,
            check=True,
        )
        file = out / name / receipt
        content = file.read_bytes()
        json.loads(content)
        report["checks"][name] = dict(
            receipt=str(file.relative_to(out)),
            sha256=hashlib.sha256(content).hexdigest(),
            wall_seconds=round(time.monotonic() - started, 2),
        )
        (out / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"{name} passed", flush=True)

    if args.suite in ("pages", "photo-reviews"):
        run("survey", "check_survey.py", "--assets")
    if args.suite != "photo-reviews":
        subprocess.run(
            [sys.executable, tools / "fetch_robot_meshes.py", "g1"], check=True
        )
    if args.suite == "photo-reviews":
        for provider in ["codex", "octos-deepseek"]:
            run(
                provider,
                "check_asset_suggestions.py",
                "--provider",
                provider,
                "--survey",
                out / "survey/state/aisle/survey",
                python=os.environ.get("DOROBOT_AISLE_PYTHON", sys.executable),
            )
    elif args.suite == "skill-qualification":
        for robot in [args.qualification_robot] if args.qualification_robot else ["G1", "Nano"]:
            source = os.environ.get("DOROBOT_QUALIFICATION_SOURCE_" + robot.upper())
            extra = ["--source", source] if source else []
            run(robot.lower(), "check_skill_qualification.py", "--robot", robot, *extra)
    elif args.suite == "spec-skills":
        for robot in ("G1", "Nano"):
            run(robot.lower(), "check_spec_skills.py", "--robot", robot)
    elif args.suite == "scene-agents":
        for provider in ("codex", "octos-deepseek"):
            run(provider, "check_scene_proposals.py", "--provider", provider)
    elif args.suite == "g1-revision":
        run("g1-revision", "check_g1_revision.py", "--provider", args.provider)
    elif args.suite == "experiment-worker":
        for robot in ("G1", "Nano"):
            run(
                robot.lower(),
                "check_scene_ranges.py",
                "--robot",
                robot,
                "--worker",
                args.worker,
                "--incompatible-worker",
                args.incompatible_worker,
            )
    elif args.suite == "scene-ranges":
        run("ranges", "check_scene_ranges.py")
        run("nano-ranges", "check_scene_ranges.py", "--robot", "Nano")
    else:
        report["nano_provider"] = args.provider
        run("scene", "check_scene_editor.py")
        run("g1", "check_start_areas.py")
        run("ranges", "check_scene_ranges.py")
        run("nano", "check_experiment.py", "--provider", args.provider)
        batch_id = json.loads((out / "nano/verification.json").read_bytes())["batch_id"]
        batch = out / "nano/state/aisle/experiments" / batch_id
        run("evidence", "check_trial_evidence.py", "--source-batch", batch)
        run(
            "channels",
            "check_trial_evidence.py",
            "--source-batch",
            batch,
            "--channels-only",
            receipt="channel-verification.json",
        )
    if (
        hashlib.sha256(studio.read_bytes()).hexdigest()
        != report["studio_binary_sha256"]
    ):
        raise RuntimeError("The lease executable changed during page qualification")
    report["binary_verified_after_checks"] = True
    (out / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Passed {', '.join(report['checks'])} at {head[:12]}", flush=True)


if __name__ == "__main__":
    main()
