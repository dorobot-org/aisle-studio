"""Qualify agent scene edits in native Studio and measure the edited furniture in Nexus."""

import argparse
import hashlib
import json
import math
from pathlib import Path

from check_experiment import experiment_fixture
from check_scene_editor import PANE
from studio_native import NativeStudio

REQUEST = "Shift the chair 20 cm north and turn it 30 degrees counterclockwise. Keep everything else where it is."


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for arg in ("studio", "remote", "session"):
        cli.add_argument("--" + arg, type=Path, required=True)
    cli.add_argument("--backend", choices=["headless", "offscreen"], default="headless")
    cli.add_argument("--provider", choices=["codex", "octos-deepseek"], required=True)
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    survey = folder / "state/aisle/survey"
    original = experiment_fixture(survey)[0]["scene"]
    scenes = survey / "scenes" / original["id"]
    old_sha = sha(scenes / "1.json")
    app = NativeStudio(args, folder)
    provider = "Codex" if args.provider == "codex" else "DeepSeek via Octos"

    def show(key, text, timeout=35):
        return app.wait(
            lambda s: text in s["controls"].get(PANE + key, {}).get("text", ""),
            key + " showing " + text,
            timeout,
        )

    def click(key):
        if key in {"update", "yaw"}:
            app.scroll_to(PANE + key, PANE + "inspector")
        app.click(PANE + key)

    def choose(key, index, expected):
        click(key)
        for event in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + event, "wait=1")
        return app.wait(
            lambda s: expected
            in (s["controls"].get(PANE + key, {}).get("selected_label") or ""),
            key + " showing " + expected,
        )

    def edit(key, value):
        click(key)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + value, "wait=1")

    def grab(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    def page():
        app.click("tabs.plugin_tab_aisle_scene")
        app.click("aisle_scene.composer_mode")

    with app.launch():
        page()
        show("header", "5 objects")
        choose("proposal_provider", 1 if args.provider == "codex" else 2, provider)
        edit("instruction", REQUEST)
        click("propose")
        show("reference_prompt", "chair", 180)
        assert "Left chair" in app.control(PANE + "reference_choices")["selected_label"]
        grab("choose-chair")
        choose("reference_choices", 1, "Right chair")
        click("confirm_reference")
        show("summary", "1 changed")
        show("selected_name", "Right chair")
        show("source_label", provider)
        assert math.isclose(float(app.control(PANE + "y")["text"]), -1.5)
        assert math.isclose(float(app.control(PANE + "yaw")["text"]), 30)
        edit("yaw", "37")
        click("update")
        show("feedback", "Preview pose updated")
        grab("review-pose")
        assert list(scenes.glob("*.json")) == [scenes / "1.json"]
        click("apply")
        show("feedback", "Proposal applied")
        click("undo")
        show("header", "Saved")
        assert math.isclose(float(app.control(PANE + "y")["text"]), -1.7)
        assert math.isclose(float(app.control(PANE + "yaw")["text"]), 0)
        click("redo")
        show("header", "Unsaved edits")
        show("source_label", provider)
        assert math.isclose(float(app.control(PANE + "yaw")["text"]), 37)
        click("save")
        show("feedback", "Scene revision saved")
    edited = json.loads((scenes / "2.json").read_bytes())
    (source,) = edited["edit_history"]
    assert source["provider"] == args.provider and source["model"]
    assert source["affected"] == source["operator_adjusted"] == ["chair-right"]
    assert all(binding[1] == "chair-right" for binding in source["bindings"])
    assert source["instruction"] == REQUEST and source["proposed_operations"]
    for key in ("request_sha256", "reply_sha256"):
        assert len(bytes.fromhex(source[key])) == 32
    assert len(original["instances"]) == len(edited["instances"])
    for before, after in zip(original["instances"], edited["instances"]):
        if before["id"] == "chair-right":
            assert after == before | {"position_m": [1.2, -1.5, 0.0], "yaw_deg": 37.0}
        else:
            assert after == before
    with app.launch():
        page()
        show("header", "Revision 2")
        show("source_label", source["model"])
        # Discard a second preview and bind the saved revision via the native Task page.
        edit("instruction", "Move the left chair beside the coffee table")
        click("propose")
        show("summary", "Preview")
        click("cancel")
        show("header", "Saved")
        app.click("tabs.plugin_tab_aisle_task")
        app.click("aisle_task.g1")
        app.click("aisle_task.select_scene")
        app.click("aisle_task.project_task.activate")
        app.wait(
            lambda s: "saved"
            in s["controls"].get("aisle_task.project_task.result", {}).get("text", ""),
            "bind the edited room",
        )
        task_sha = (survey / "active-task-g1").read_text()
        frozen = survey / "tasks" / (task_sha + ".json")
        assert (
            sha(frozen) == task_sha
            and json.loads(frozen.read_bytes())["scene"] == edited
        )
        app.click("aisle_task.select_scene")
        app.click("aisle_task.reference")
        app.wait(
            lambda s: (s["state"]["controller"] or {}).get("bound_task_sha256")
            == task_sha
            and not s["state"]["busy"],
            "reference standing controller",
            150,
        )
        app.click("tabs.plugin_tab_aisle_run")
        app.click("aisle_run.start_run")
        app.wait(
            lambda s: not s["state"]["busy"]
            and (s["state"].get("result") or {}).get("bound_task_sha256") == task_sha,
            "standing reach in edited room",
            180,
        )
    (result_path,) = [
        p
        for p in (folder / "state/aisle").glob("*/result.json")
        if json.loads(p.read_bytes()).get("bound_task_sha256") == task_sha
    ]
    result = json.loads(result_path.read_bytes())
    assert (
        result["task_success"]
        and result["both_feet_loaded_steps"] == result["physics_ticks"] == 2000
    )
    assert result["wrist_reach"]["passed"] and result["disturbance"]["passed"]
    chairs = {}
    for body in result["scene_execution"]["bodies"]:
        key = body["instance"]
        if key in ("chair-left", "chair-right"):
            xyz = body["measured_center_m"]
            w, _, _, z = body["measured_quat_wxyz"]
            yaw = math.degrees(2 * math.atan2(z, w)) % 360
            right = key == "chair-right"
            assert abs(xyz[0] - (1.2 if right else -1.2)) < 1e-5
            assert abs(xyz[1] - (-1.5 if right else -1.7)) < 1e-5
            assert abs(math.remainder(yaw - (37 if right else 0), 360)) < 1e-4
            chairs[key] = dict(center_m=xyz, yaw_deg=yaw)
    assert len(chairs) == 2 and sha(scenes / "1.json") == old_sha
    proof = dict(
        backend=args.backend,
        agent_source=source,
        original_sha256=old_sha,
        saved_revision_sha256=sha(scenes / "2.json"),
        task_sha256=task_sha,
        result_sha256=sha(result_path),
        nexus_chairs=chairs,
        robot="G1",
        both_feet_loaded_steps=result["both_feet_loaded_steps"],
        reached=result["wrist_reach"]["passed"],
        bounded_impact_passed=result["disturbance"]["passed"],
        unchanged_instances=4,
        original_preserved=True,
        undo_redo_verified=True,
        discard_verified=True,
    )
    (folder / "verification.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(
        f"{provider} edit persisted, right chair measured at 37°, G1 passed 2,000 loaded steps"
    )


if __name__ == "__main__":
    main()
