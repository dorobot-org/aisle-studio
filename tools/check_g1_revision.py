"""Revise a measured failed G1 standing trial through Task and reopen its saved result."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
from zipfile import ZipFile

from studio_native import NativeStudio, ROOT

REQUEST = "Keep the saved left shoulder reach. Try a 30-degree left waist turn with neutral pitch for the next simulation."


def digest(data):
    return hashlib.sha256(data).hexdigest()


def first_preparation(args, folder, survey, sha):
    state = folder / "state/aisle"
    shutil.copytree(survey, state / "survey")
    app = NativeStudio(args, folder)
    with app.launch():
        app.click("aisle_task.g1")
        app.click("aisle_task.turned_stance")
        app.click("aisle_task." + args.provider)
        prepared = app.wait(
            lambda s: (s["state"].get("controller") or {}).get("bound_task_sha256")
            == sha
            and not s["state"]["busy"],
            "first scene-bound reach",
            180,
        )["state"]["controller"]
        note = prepared["preparation_instruction"]
        assert "30-degree left waist turn" in note and "previous_trial" not in prepared
        assert prepared["trunk_yaw_rad"] == 0.52359878
        app.wait(
            lambda s: note in s["controls"]["aisle_task.detail"]["text"],
            "visible prepared posture",
        )
        app.click("tabs.plugin_tab_aisle_run")
        app.click("aisle_run.start_run")
        app.wait(
            lambda s: not s["state"]["busy"]
            and (s["state"].get("result") or {}).get("bound_task_sha256") == sha,
            "first reach verdict",
            180,
        )
    rows = list(state.glob("*-G1-run/result.json"))
    assert len(rows) == 1
    measured = json.loads(rows[0].read_bytes())
    assert measured["task_success"] and measured["both_feet_loaded_steps"] == 2000
    assert abs(measured["settled_trunk_yaw_rad"] - math.pi / 6) < 0.02
    return dict(
        operator_request=note,
        previous_runs=0,
        passed=measured["task_success"],
        controller_sha256=measured["controller_hash"],
        settled_yaw_rad=measured["settled_trunk_yaw_rad"],
    )


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for key in ("studio", "session", "remote"):
        cli.add_argument("--" + key, required=True, type=Path)
    cli.add_argument("--backend", choices=["headless", "offscreen"], default="headless")
    cli.add_argument("--provider", choices=["octos", "codex"], required=True)
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    state = folder / "state/aisle"
    prior = state / "1000-G1-run"
    prior.mkdir(parents=True)
    archive = (
        ROOT
        / "docs/product/aisle-studio/environment/g1-verification/matched-upright/matched.zip"
    )
    with ZipFile(archive) as z:
        for name in [
            "controller.json",
            "bound-task.json",
            "result.json",
            "source.json",
        ]:
            (prior / name).write_bytes(z.read("case-014/" + name))
    original = {f.name: digest(f.read_bytes()) for f in prior.iterdir()}
    failed = json.loads((prior / "result.json").read_bytes())
    sha = original["bound-task.json"]
    assert failed["task_success"] is False and failed["bound_task_sha256"] == sha
    assert failed["controller_hash"] == original["controller.json"]
    survey = state / "survey"
    (survey / "tasks").mkdir(parents=True)
    (survey / "tasks" / (sha + ".json")).write_bytes(
        (prior / "bound-task.json").read_bytes()
    )
    (survey / "active-task-g1").write_text(sha)
    first = first_preparation(args, folder / "first-preparation", survey, sha)
    app = NativeStudio(args, folder)
    with app.launch():
        app.click("tabs.plugin_tab_aisle_task")
        app.click("aisle_task.instruction")
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + REQUEST, "wait=1")
        app.click(
            "aisle_task.revise"
            if args.provider == "octos"
            else "aisle_task.revise_codex"
        )
        app.wait(
            lambda s: (s["state"].get("controller") or {}).get("revision_instruction")
            == REQUEST
            and not s["state"]["busy"],
            "saved revision",
            180,
        )
        app.wait(
            lambda s: REQUEST in s["controls"]["aisle_task.detail"]["text"],
            "visible revision request",
        )
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / "revision.png"))
        app.click("tabs.plugin_tab_aisle_run")
        app.click("aisle_run.start_run")
        app.wait(
            lambda s: not s["state"]["busy"]
            and (s["state"].get("result") or {}).get("controller_hash")
            not in (None, original["controller.json"]),
            "revised reach measurements",
            180,
        )
        app.click("tabs.plugin_tab_aisle_evidence")
    updated = next(p for p in state.glob("*-G1-run/result.json") if p.parent != prior)
    result = json.loads(updated.read_bytes())
    prepared = next(state.glob("*-G1-prepare/controller.json"))
    control = json.loads(prepared.read_bytes())
    assert control["bound_task_sha256"] == result["bound_task_sha256"] == sha
    assert control["previous_trial"]["controller_hash"] == original["controller.json"]
    assert (
        control["revision_instruction"] == REQUEST
        and control["task"]
        == json.loads((prior / "bound-task.json").read_bytes())["instruction"]
    )
    assert (
        control["previous_trial"]["standing_measurements"]["prior_trunk_pitch_rad"]
        == -0.10
    )
    assert (
        control["trunk_yaw_rad"] == 0.52359878
        and control.get("trunk_pitch_rad", 0) == 0
    )
    assert result["controller_hash"] == digest(prepared.read_bytes())
    assert (
        result["task_success"]
        and result["within_limits"]
        and result["both_feet_loaded_steps"] == 2000
    )
    assert abs(result["settled_trunk_yaw_rad"] - math.pi / 6) < 0.02
    assert not result["scene_contacts"] and result["root_pose_writes"] == 0
    assert all(
        digest((prior / name).read_bytes()) == val for name, val in original.items()
    )
    with app.launch():
        app.click("tabs.plugin_tab_aisle_task")
        app.wait(
            lambda s: REQUEST in s["controls"]["aisle_task.detail"]["text"],
            "restored revision",
        )
        app.click("tabs.plugin_tab_aisle_evidence")
        app.wait(
            lambda s: (s["state"].get("result") or {}).get("controller_hash")
            == result["controller_hash"],
            "restored verdict",
        )
    receipt = dict(
        first_preparation=first,
        provider=control["provider"],
        model=control["policy_model"],
        backend=args.backend,
        task_sha256=sha,
        prior_controller_sha256=original["controller.json"],
        controller_sha256=result["controller_hash"],
        prior_impact_case=14,
        prior_passed=False,
        prior_stance=control["previous_trial"]["standing_measurements"],
        revision_instruction=REQUEST,
        nominal_impact=result["disturbance"],
        reach=result["wrist_reach"],
        settled_trunk_yaw_rad=result["settled_trunk_yaw_rad"],
        two_foot_steps=result["both_feet_loaded_steps"],
        passed=result["task_success"],
        original_preserved=True,
        restored=True,
    )
    (folder / "verification.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(
        f"{control['policy_model']} revision restored, measured waist turn {math.degrees(result['settled_trunk_yaw_rad']):.2f} degrees",
        flush=True,
    )


if __name__ == "__main__":
    main()
