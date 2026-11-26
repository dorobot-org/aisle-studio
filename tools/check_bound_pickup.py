"""Native scene-to-task acceptance fixture, including two visually distinct pickup candidates."""

import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INSTRUCTION = "Pick up the cube and put it on the coffee table."


def fixture(root):
    receipt = json.loads(
        (
            ROOT
            / "docs/product/aisle-studio/environment/survey-verification/offscreen.json"
        ).read_bytes()
    )
    template = next(
        a
        for a in receipt["assets"]
        if a["name"] == "Coffee table" and a["revision"] == 1
    )

    def prop(value):
        return dict(value=value, provenance=dict(source="operator"))

    def asset(name, appearance, dims, colliders, surfaces, movable=False):
        a = copy.deepcopy(template)
        a.update(
            id="asset-" + appearance,
            name=name,
            revision=1,
            category="Object" if movable else "Furniture",
            photos={},
            geometry=dict(colliders=colliders, surfaces=surfaces),
        )
        a["dimensions"] = [
            dict(value=v, unit="m", provenance=dict(source="operator")) for v in dims
        ]
        a["physics"].update(
            appearance=prop(appearance),
            shape="box" if movable else "four_leg_table",
            graspable=prop(movable),
            movable=prop(movable),
            mass_kg=prop(0.032 if movable else None),
            material=prop("Painted wood" if movable else "Oak"),
            friction=prop(0.7),
            surface_name=prop("Landing area"),
            top_thickness_m=prop(0.08),
            leg_width_m=prop(0.08),
        )
        path = root / "assets" / a["id"] / "1.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps(a))
        return a

    def box(name, pos, size):
        return dict(name=name, center_m=pos, size_m=size)

    h = 0.5743
    bodies = [box("top", [0, 0, h - 0.04], [1.4, 0.8, 0.08])]
    for i, (x, y) in enumerate(
        [(-0.66, -0.36), (0.66, -0.36), (-0.66, 0.36), (0.66, 0.36)]
    ):
        bodies.append(box(f"leg-{i}", [x, y, (h - 0.08) / 2], [0.08, 0.08, h - 0.08]))
    top = asset(
        "Coffee table",
        "oak",
        [1.4, 0.8, h],
        bodies,
        [dict(name="Landing area", center_m=[0, 0, h], size_m=[1.4, 0.8])],
    )

    def cube(color):
        return asset(
            color.title() + " cube",
            color,
            [0.04] * 3,
            [box("body", [0, 0, 0.02], [0.04] * 3)],
            [],
            True,
        )

    red, blue = cube("red"), cube("blue")

    def item(a, x, y):
        return dict(
            id="object-" + a["physics"]["appearance"]["value"],
            name=a["name"],
            asset=dict(id=a["id"], revision=1),
            position_m=[x, y, 0.0],
            yaw_deg=0.0,
            support=None,
        )

    scene = dict(
        id="scene-living-room",
        revision=1,
        parent=None,
        name="Surveyed living room",
        room_m=[12.0, 10.0],
        instances=[item(top, 0.0, 1.0), item(red, 0.73, 0.20), item(blue, 0.84, -0.32)],
    )
    path = root / "scenes" / scene["id"] / "1.json"
    path.parent.mkdir(parents=True)
    path.write_text(json.dumps(scene))
    return scene, [top, red, blue]


def main():
    import argparse, hashlib, math
    from studio_native import NativeStudio

    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--studio", required=True, type=Path)
    cli.add_argument("--remote", required=True, type=Path)
    cli.add_argument("--session", required=True, type=Path)
    cli.add_argument(
        "--backend", choices=["offscreen", "headless"], default="offscreen"
    )
    cli.add_argument(
        "--bindings-only",
        action="store_true",
        help="Exercise binding and revision changes without starting Nexus",
    )
    cli.add_argument(
        "--provider", choices=["reference", "codex", "octos"], default="reference"
    )
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    root = folder / "state/aisle/survey"
    scene, assets = fixture(root)
    app = NativeStudio(args, folder)
    task = "aisle_task.project_task."
    edit = "aisle_scene.composer."

    def show(key, value):
        return app.wait(
            lambda s: value in s["controls"].get(key, {}).get("text", ""),
            f"{key} to contain {value}",
        )

    def choose(key, delta):
        app.click(key)
        x, y, w, h = app.control(key)["rect"]
        app.act("click", "w=0", f"x={x+25}", f"y={y+h/2+delta*30}", "wait=1")

    def grab(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    def change(key, value):
        app.click(key)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + value, "wait=1")

    def editor():
        app.click("tabs.plugin_tab_aisle_scene")
        state = app.wait(lambda s: s["state"]["page"] == "aisle_scene")
        if state["controls"]["aisle_scene.composer_mode"].get("enabled") is not False:
            app.click("aisle_scene.composer_mode")

    def return_to_task():
        app.click("aisle_task.select_scene")
        show("aisle_task.binding_status", "Surveyed living room")

    measured = {}
    with app.launch():
        editor()
        app.click("aisle_scene.create_bound_task")
        show(task + "result", "Multiple graspable objects")
        assert not app.control(task + "selection")["text"].startswith(
            "Pickup · Red cube"
        )
        choose(task + "pickup", 1)
        show(task + "selection", "Red cube")
        grab("bound-pickup")
        app.click(task + "activate")
        show(task + "result", "saved")
        digest = (root / "active-task").read_text()
        frozen = (root / "tasks" / (digest + ".json")).read_bytes()
        snapshot = json.loads(frozen)
        assert hashlib.sha256(frozen).hexdigest() == digest
        assert snapshot["pickup"] == "object-red" and snapshot["placement"] == dict(
            instance="object-oak", surface="Landing area"
        )
        return_to_task()
        if not args.bindings_only:
            if args.provider != "reference":
                app.click("aisle_task.onboard")
            app.click("aisle_task." + args.provider)
            status = app.wait(
                lambda s: not s["state"]["busy"]
                and (
                    s["state"]["controller"].get("bound_task_sha256") == digest
                    if isinstance(s["state"]["controller"], dict)
                    else False
                )
                or (not s["state"]["busy"] and "fail" in s["state"]["status"].lower()),
                "bound controller preparation",
                timeout=300,
            )["state"]
            assert status["controller"]["bound_task_sha256"] == digest, status["status"]
            app.click("tabs.plugin_tab_aisle_run")
            app.click("aisle_run.start_run")
            app.wait(lambda s: s["state"]["busy"], "Nexus launch")
            status = app.wait(
                lambda s: not s["state"]["busy"], "Nexus result", timeout=1500
            )["state"]
            result = status["result"]
            assert result.get("bound_task_sha256") == digest, status["status"]
            measured = result["scene_execution"]
            assert (
                result["task_success"]
                and result["wheel_supported_ticks"] == result["physics_ticks"]
                and not result["robot_furniture_contacts"]
                and result["actuator_audit"]["passed"]
                and not result["root_pose_writes"]
            ), result
            assert {b["instance"] for b in measured["bodies"] if b["instance"]} == {
                i["id"] for i in scene["instances"]
            }
            actual = {b["name"]: b for b in measured["bodies"]}
            assert actual[measured["cube_body"]]["instance"] == "object-red"
            support = actual[measured["support_body"]]
            assert support["instance"] == "object-oak" and support["collider"] == "top"
            assert math.dist(support["measured_center_m"], [0, 1, 0.50]) < 1e-5
            assert (
                len([v for v in actual.values() if v["instance"] == "object-oak"]) == 5
            )
            cube = result["cubes"]["red"]
            assert (
                cube["table_contact_tail_ticks"] == 100
                and cube["detached"]
                and cube["settle_motion_m"] < 0.003
                and cube["opposing_finger_airborne_ticks"] >= 20
            )
            bystanders = [b for b in actual.values() if b["instance"] == "object-blue"]
            assert (
                len(bystanders) == 1
                and math.dist(bystanders[0]["measured_center_m"][:2], [0.84, -0.32])
                < 0.01
            )
            trial = next(
                p.parent
                for p in (folder / "state/aisle").glob("*/result.json")
                if json.loads(p.read_bytes()).get("bound_task_sha256") == digest
            )
            assert (
                hashlib.sha256((trial / "bound-task.json").read_bytes()).hexdigest()
                == digest
            )
            assert (
                hashlib.sha256((trial / "controller.json").read_bytes()).hexdigest()
                == result["controller_hash"]
            )
            calibration = json.loads((trial / "calibration.json").read_bytes())
            assert (
                set(calibration["cameras"]) == {"head", "left_wrist", "right_wrist"}
                and "scene" not in calibration
                and "specs" not in calibration
            )
            measured = {
                "task_sha256": digest,
                "controller_sha256": result["controller_hash"],
                "provider": args.provider,
                "physics_ticks": result["physics_ticks"],
                "wheel_supported_ticks": result["wheel_supported_ticks"],
                "max_joint_excursion_rad": result["max_joint_excursion_rad"],
                "robot_furniture_contacts": result["robot_furniture_contacts"],
                "cube": cube,
                "runtime": measured,
                "calibration_channels": list(calibration["cameras"]),
            }
            app.click("tabs.plugin_tab_aisle_evidence")
            grab("evidence")
        editor()
        app.scroll_to(edit + "objects", edit + "inspector")
        choose(edit + "objects", 1)
        show(edit + "selected_name", "Red cube")
        app.scroll_to(edit + "selected_name", edit + "inspector")
        change(edit + "selected_name", "Renamed red cube")
        app.scroll_to(edit + "update", edit + "inspector")
        app.click(edit + "update")
        app.click(edit + "save")
        show(edit + "header", "Revision 2")
        app.click("aisle_scene.create_bound_task")
        show(task + "selection", "Renamed red cube")
        show(task + "scene_label", "revision 2")
        app.click(task + "activate")
        show(task + "result", "saved")
        renamed = (root / "active-task").read_text()
        assert (
            json.loads((root / "tasks" / (renamed + ".json")).read_bytes())["pickup"]
            == "object-red"
            and renamed != digest
        )
        editor()
        app.scroll_to(edit + "remove", edit + "inspector")
        app.click(edit + "remove")
        app.click(edit + "save")
        show(edit + "header", "Revision 3")
        app.click("aisle_scene.create_bound_task")
        show(task + "result", "deleted")
        grab("broken-binding")
        state = app.wait(
            lambda s: s["controls"][task + "activate"].get("enabled") is False
        )
        assert (root / "active-task").read_text() == renamed
        app.click(task + "earlier_scene")
        show(task + "selection", "Renamed red cube")
        app.click(task + "earlier_scene")
        show(task + "selection", "Red cube")
        app.click(task + "activate")
        show(task + "result", "saved")
        assert (root / "active-task").read_text() == digest
    with app.launch():
        app.click("tabs.plugin_tab_aisle_task")
        app.click("aisle_task.select_scene")
        show(task + "selection", "Red cube")
        show(task + "scene_label", "revision 1")
        assert (
            hashlib.sha256(
                (root / "tasks" / (digest + ".json")).read_bytes()
            ).hexdigest()
            == digest
        )
    (folder / "verification.json").write_text(
        json.dumps(
            dict(
                backend=args.backend,
                task_sha256=digest,
                renamed_task_sha256=renamed,
                pickup="object-red",
                restored_scene_revision=1,
                deleted_in_revision=3,
                measurement=measured,
            ),
            indent=2,
        )
        + "\n"
    )
    print(
        f"{args.backend}: selected one of two cubes, verified renamed and deleted bindings, restored v1."
        + (" Nexus measured grasp, release, and settled support." if measured else "")
    )


if __name__ == "__main__":
    main()
