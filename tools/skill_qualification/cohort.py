"""Register and run held-out Nexus qualifications through the selected Octos package."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import fcntl
import json
from pathlib import Path
import re
import subprocess
import sys
from tempfile import TemporaryDirectory

TOOLS = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(TOOLS / "aisle_workbench"), str(TOOLS / "aisle-validation")]

import g1_reach_suite as g1
import nano_transfer_suite as nano
import spec_runtime
from spec_catalog import digest, encode, save_atomic


def read(path):
    return json.loads(path.read_bytes())


def sha(path):
    return digest(path.read_bytes())


@contextmanager
def exclusive(path):
    with path.open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise ValueError(
                "Another qualification operation is still running"
            ) from exc
        yield lock


def register(state, source):
    plan = read(source)
    robot = plan["robot"]
    pin = plan.get("spec_skill")
    if not pin or pin != spec_runtime.lock_selection(pin["package"], robot):
        raise ValueError("Prepare a controller with the installed Octos skill selected")
    if "scene_observation" in plan:
        raise ValueError(
            "This protocol varies the environment and requires fresh observations"
        )
    if robot == "Nano" and "bound_task_sha256" in plan:
        raise ValueError(
            "Select the three-cube reference living room for the 50 hidden layouts"
        )
    state.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(dir=state) as staging:
        directory = Path(staging) / "suite"
        if robot == "Nano":
            nano.prepare(directory, source)
        elif robot == "G1":
            g1.prepare(directory, source)
        else:
            raise ValueError("Unsupported qualification robot")
        manifest = read(directory / "suite.json")
        registration = dict(
            version=1,
            robot=robot,
            protocol="nano-three-cube-v2"
            if robot == "Nano"
            else "g1-standing-reach-v1",
            manifest_sha256=sha(directory / "suite.json"),
            controller_sha256=sha(source),
            spec_skill=pin,
            observation_inputs=spec_runtime.observation_inputs(plan),
            cases=manifest["cases"],
            required=45 if robot == "Nano" else 20,
            runner_sha256=sha(Path(__file__)),
        )
        identity = digest(encode(registration))
        save_atomic(directory / "registration.json", registration)
        directory.rename(state / identity)
    return identity


def directory_for(state, identity):
    if re.fullmatch(r"[0-9a-f]{64}", identity or "") is None:
        raise ValueError("Select a registered qualification")
    return state / identity


def frozen(directory):
    reg = read(directory / "registration.json")
    manifest = read(directory / "suite.json")
    plan = read(directory / "controller.json")
    robot = reg["robot"]
    if (
        digest(encode(reg)) != directory.name
        or sha(directory / "suite.json") != reg["manifest_sha256"]
        or sha(directory / "controller.json") != reg["controller_sha256"]
        or manifest["controller_sha256"] != reg["controller_sha256"]
        or plan["robot"] != robot
        or plan["spec_skill"] != reg["spec_skill"]
        or spec_runtime.lock_selection(reg["spec_skill"]["package"], robot)
        != reg["spec_skill"]
        or spec_runtime.observation_inputs(plan) != reg["observation_inputs"]
        or sha(Path(__file__)) != reg["runner_sha256"]
    ):
        raise ValueError(
            "Registration, installed package, runner or controller changed"
        )
    if robot == "G1":
        g1.manifest(directory, reg["manifest_sha256"])
        openings = read(directory / "openings.json")
        valid = (
            "matched_manifest_sha256" not in manifest
            and reg["required"] == 20
            and len(openings) == reg["cases"] == 20
            and [g1.commitment(x) for x in openings] == manifest["commitments"]
            and manifest["inputs"] == g1.inputs()
        )
    elif robot == "Nano":
        openings = read(directory / "private-layouts.json")
        valid = (
            manifest["split"] == "hidden"
            and len(openings) == reg["cases"] == 50
            and reg["required"] == manifest["success_requirement"] == 45
            and len({x["seed"] for x in openings}) == 50
            and [nano.signature(x) for x in openings] == manifest["commitments"]
            and manifest["inputs"] == nano.inputs()
        )
    else:
        valid = False
    if not valid:
        raise ValueError("Held-out cases, profile or frozen evaluator inputs changed")
    return reg, manifest, openings


def run(directory, jobs):
    with exclusive(directory / "operation.lock") as lock:
        reg, manifest, openings = frozen(directory)

        def launch(item):
            index, opening = item
            case = directory / f"case-{index:03}"
            log = case.with_suffix(".log")
            if log.exists():
                return  # Every attempted slot remains in the denominator, including interruptions.
            args = [
                sys.executable,
                str(TOOLS / "aisle_workbench/worker.py"),
                "run",
                "--robot",
                reg["robot"],
                "--controller",
                str(directory / "controller.json"),
                "--out",
                str(case),
            ]
            if reg["robot"] == "G1":
                recipe = directory / f"impact-{index:03}.json"
                save_atomic(recipe, opening["recipe"])
                args += [
                    "--fixture",
                    "impact",
                    "--impact-aim",
                    manifest["impact_aim"],
                    "--impact-recipe",
                    str(recipe),
                ]
            else:
                args += [
                    "--seed",
                    str(opening["seed"]),
                    "--observations-only",
                    "--observation-period-s",
                    "5",
                ]
            with log.open("xb") as stream:
                task = subprocess.run(
                    args,
                    stdout=stream,
                    stderr=subprocess.STDOUT,
                    pass_fds=(lock.fileno(),),
                )
            if reg["robot"] == "G1" and case.is_dir():
                save_atomic(case / "impact.json", opening["recipe"])
            save_atomic(
                directory / f"attempt-{index:03}.json", {"exit_code": task.returncode}
            )

        with ThreadPoolExecutor(max_workers=jobs) as pool:
            list(pool.map(launch, enumerate(openings)))
        frozen(directory)


def status(directory):
    reg = read(directory / "registration.json")
    report = directory / "verification.json"
    return dict(
        id=directory.name,
        robot=reg["robot"],
        cases=reg["cases"],
        required=reg["required"],
        started=len(list(directory.glob("case-*.log"))),
        finished=len(list(directory.glob("attempt-*.json"))),
        report=read(report) if report.exists() else None,
    )


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=["register", "run", "status"])
    p.add_argument("--state", type=Path, required=True)
    p.add_argument("--controller", type=Path)
    p.add_argument("--id")
    p.add_argument("--jobs", type=int, choices=range(1, 9), default=2)
    args = p.parse_args()
    if args.action == "register":
        args.id = register(args.state, args.controller)
    directory = directory_for(args.state, args.id)
    if args.action == "run":
        run(directory, args.jobs)
    print(json.dumps(status(directory)))
