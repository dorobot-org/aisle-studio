"""Author a floor and tabletop start region, then measure both in two Nexus runs."""

import argparse
import hashlib
import json
import math
from pathlib import Path

from check_experiment import experiment_fixture
from studio_native import NativeStudio


def read(path):
    return json.loads(path.read_bytes())


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for key in ("studio", "remote", "session"):
        cli.add_argument("--" + key, type=Path, required=True)
    cli.add_argument(
        "--backend", choices=["headless", "offscreen"], default="offscreen"
    )
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    survey = folder / "state/aisle/survey"
    initial, _ = experiment_fixture(survey)
    versions = survey / "scenes" / initial["scene"]["id"]
    original = (versions / "1.json").read_bytes()
    app = NativeStudio(args, folder)
    editor = "aisle_scene.composer."
    run = "aisle_run.experiment_pane.exp_"
    inspector, form = editor + "inspector", run + "form"

    def show(id_, text, timeout=45):
        return app.wait(
            lambda s: text in s["controls"].get(id_, {}).get("text", ""),
            id_ + " · " + text,
            timeout=timeout,
        )

    def click(id_, scroll=None):
        if scroll:
            app.scroll_to(id_, scroll)
        app.click(id_)

    def field(id_, value, scroll):
        click(id_, scroll)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + str(value), "wait=1")

    def choose(id_, index, scroll):
        click(id_, scroll)
        for key in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + key, "wait=1")

    def editor_page():
        click("tabs.plugin_tab_aisle_scene")
        click("aisle_scene.composer_mode")

    def set_bounds(values):
        for key, value in zip(
            ("area_x_min", "area_x_max", "area_y_min", "area_y_max"), values
        ):
            field(editor + key, value, inspector)
        click(editor + "area_save", inspector)

    def capture(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    with app.launch():
        editor_page()
        click(editor + "open_scene")
        show(editor + "header", "5 objects")
        field(editor + "yaw", 20, inspector)
        click(editor + "update", inspector)
        choose(editor + "objects", 1, inspector)
        set_bounds([0.65, 1.0, -0.50, -0.25])
        show(editor + "area_status", "Area defined")
        choose(editor + "objects", 2, inspector)
        choose(editor + "area_support", 1, inspector)
        set_bounds([0.15, 0.90, -0.20, 0.10])
        show(editor + "feedback", "beyond the support")
        field(editor + "area_x_max", 0.45, inspector)
        click(editor + "area_save", inspector)
        show(editor + "area_status", "Area defined")
        capture("tabletop-area")
        click(editor + "save")
        show(editor + "feedback", "Scene revision saved")
    assert (versions / "1.json").read_bytes() == original
    scene = read(versions / "2.json")
    areas = {
        v["id"]: v["start_area"] for v in scene["instances"] if v.get("start_area")
    }
    assert areas["object-blue"]["support"] == {
        "instance": "object-oak",
        "surface": "Landing area",
    }
    assert areas["object-red"]["support"] is None
    assert scene["instances"][0]["yaw_deg"] == 20
    with app.launch():
        click("aisle_task.g1")
        click("aisle_task.select_scene")
        click("aisle_task.project_task.activate")
        show("aisle_task.project_task.result", "saved")
        task_hash = (survey / "active-task-g1").read_text()
        task = read(survey / "tasks" / (task_hash + ".json"))
        assert task["scene"] == scene
        click("aisle_task.select_scene")
        click("aisle_task.reference")
        app.wait(
            lambda s: (s["state"].get("controller") or {}).get("bound_task_sha256")
            == task_hash
            and not s["state"]["busy"],
            "G1 prepared",
            timeout=150,
        )
        click("tabs.plugin_tab_aisle_run")
        click("aisle_run.experiment_mode")
        field(run + "count", 2, form)
        click(run + "cameras", form)
        click(run + "preview")
        show(run + "preview_status", "Seed 42")
        assert "Landing area" not in app.control(run + "preview_values")["text"]
        click(run + "preview")
        for i in range(2):
            choose(run + "area", i, form)
            click(run + "area_toggle", form)
            show(run + "area_toggle", "Disable")
        click(run + "preview")
        previews = []
        for i in range(2):
            if i:
                click(run + "preview_next")
            state = app.wait(
                lambda snap: f"Seed {42+i} ·"
                in snap["controls"][run + "preview_status"]["text"]
                and "Blue cube on Landing area"
                in snap["controls"][run + "preview_values"]["text"],
                f"both sampled regions for seed {42+i}",
            )["controls"]
            values = state[run + "preview_values"]["text"]
            assert (
                "Red cube on floor" in values and "Blue cube on Landing area" in values
            ), values
            previews.append(state[run + "preview_status"]["text"].split(" · ")[-1])
        capture("area-preview")
        click(run + "start")
        show(run + "feedback", "Batch started")
        app.wait(
            lambda s: s["controls"]
            .get(run + "rows", {})
            .get("text", "")
            .count("completed")
            == 2,
            "two measured G1 trials",
            timeout=300,
        )
        show(run + "detail", "Blue cube on Landing area")
    with app.launch():
        click("aisle_task.g1")
        click("tabs.plugin_tab_aisle_run")
        click("aisle_run.experiment_mode")
        show(run + "detail", "Red cube on floor")
        click(run + "preview")
        show(run + "preview_status", previews[0])
        show(run + "preview_values", "Blue cube on Landing area")
    manifest = next((folder / "state/aisle/experiments").glob("*/batch.json"))
    proof = []
    for i, row in enumerate(read(manifest)["repetitions"]):
        trial = manifest.parent / "trials" / row["run_id"]
        layout = read(trial / "sampled-layout.json")
        assert (
            hashlib.sha256(
                json.dumps(layout, sort_keys=True, separators=(",", ":")).encode()
            ).hexdigest()
            == row["sample_sha256"]
        )
        assert row["sample_sha256"].startswith(previews[i])
        result = read(trial / "result.json")
        assert result["task_success"] and result["both_feet_loaded_steps"] == 2000
        bodies = result["scene_execution"]["bodies"]
        red = next(b for b in bodies if b["instance"] == "object-red")
        blue = next(b for b in bodies if b["instance"] == "object-blue")
        top = max(
            (b for b in bodies if b["instance"] == "object-oak"),
            key=lambda b: b["measured_center_m"][2],
        )
        floor = next(b for b in bodies if b["name"] == "floor")
        measured = {}
        for box, support, identifier in [
            (red, floor, "object-red"),
            (blue, top, "object-blue"),
        ]:
            pos = box["measured_center_m"]
            origin = support["measured_center_m"]
            z = origin[2] + support["size_m"][2] / 2
            assert abs(pos[2] - box["size_m"][2] / 2 - z) < 0.002, (pos, z)
            angle = math.radians(20 if box is blue else 0)
            dx, dy = pos[0] - origin[0], pos[1] - origin[1]
            xy = [
                math.cos(angle) * dx + math.sin(angle) * dy,
                -math.sin(angle) * dx + math.cos(angle) * dy,
            ]
            sampled = row["placements"][identifier]["local_xy_m"]
            assert max(abs(a - b) for a, b in zip(xy, sampled)) < 0.002, (xy, sampled)
            lo, hi = areas[identifier]["bounds_m"]
            footprint = 0.02 * (abs(math.cos(angle)) + abs(math.sin(angle)))
            assert all(
                lo[k] + footprint - 0.002 <= xy[k] <= hi[k] - footprint + 0.002
                for k in range(2)
            )
            measured[identifier] = dict(
                sampled_local_xy_m=sampled,
                measured_local_xy_m=xy,
                bottom_to_support_m=pos[2] - box["size_m"][2] / 2 - z,
            )
        proof.append(
            dict(
                seed=row["seed"],
                layout_sha256=row["sample_sha256"],
                physics_steps=result["physics_ticks"],
                placements=measured,
            )
        )
    assert proof[0]["placements"] != proof[1]["placements"]
    (folder / "verification.json").write_text(
        json.dumps(
            dict(
                backend=args.backend, task_sha256=task_hash, areas=areas, trials=proof
            ),
            indent=2,
        )
        + "\n"
    )
    print(
        f"{args.backend}: two start areas sampled and physically verified on each of two seeds"
    )


if __name__ == "__main__":
    main()
