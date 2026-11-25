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
    parser.add_argument(
        "--photo-reviews",
        action="store_true",
        help="Run the survey and both authenticated proposal workflows as a focused lease",
    )
    args = parser.parse_args()
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

    run("survey", "check_survey.py", "--assets")
    if args.photo_reviews:
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
    else:
        subprocess.run(
            [sys.executable, tools / "fetch_robot_meshes.py", "g1"], check=True
        )
        report["nano_provider"] = args.provider
        run("scene", "check_scene_editor.py")
        run("g1", "check_start_areas.py")
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
