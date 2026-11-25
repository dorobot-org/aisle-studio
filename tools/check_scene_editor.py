"""Qualify scene composition with an existing library of metric test assets."""

import argparse
import hashlib
import json
from pathlib import Path

from studio_native import NativeStudio, ROOT

PANE = "aisle_scene.composer."


def fixtures(root):
    # A recorded measured coffee table, plus three independent operator-defined box assets.
    receipt = json.loads(
        (
            ROOT
            / "docs/product/aisle-studio/environment/survey-verification/offscreen.json"
        ).read_bytes()
    )
    table = next(
        a
        for a in receipt["assets"]
        if a["name"] == "Coffee table" and a["revision"] == 1
    )
    for name, dims in [
        ("Chair", (0.42, 0.43, 0.80)),
        ("Coffee table", (0.80, 0.50, 0.45)),
        ("Cube", (0.09, 0.09, 0.09)),
        ("Ottoman", (0.62, 0.48, 0.38)),
    ]:
        doc = json.loads(json.dumps(table))
        doc.update(
            id="fixture-" + name.lower().replace(" ", "-"),
            name=name,
            category="test library",
            photos={},
        )
        doc["dimensions"] = [
            dict(value=v, unit="m", provenance=dict(source="operator")) for v in dims
        ]
        if name != "Coffee table":
            doc["geometry"] = dict(
                colliders=[
                    dict(name="body", center_m=[0, 0, dims[2] / 2], size_m=dims)
                ],
                surfaces=[],
            )
            doc["physics"]["shape"] = "box"
        if name == "Cube":
            for key, value in [
                ("movable", True),
                ("graspable", True),
                ("mass_kg", 0.09),
            ]:
                doc["physics"][key] = dict(
                    value=value, provenance=dict(source="operator")
                )
        path = root / "assets" / doc["id"] / "1.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps(doc))


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--studio", required=True, type=Path)
    cli.add_argument("--remote", required=True, type=Path)
    cli.add_argument("--backend", default="headless", choices=["headless", "offscreen"])
    cli.add_argument("--session", required=True, type=Path)
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    root = folder / "state/aisle/survey"
    fixtures(root)
    app = NativeStudio(args, folder)

    def click(key):
        if key in {
            "palette",
            "objects",
            "x",
            "y",
            "z",
            "yaw",
            "update",
            "duplicate",
            "remove",
            "selected_name",
            "add",
        }:
            app.scroll_to(PANE + key, PANE + "inspector")
        app.click(PANE + key)

    def show(key, text):
        app.wait(
            lambda s: text in s["controls"].get(PANE + key, {}).get("text", ""),
            f"{key} to contain {text}",
        )

    def edit(key, text):
        click(key)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + text, "wait=1")

    def choose(key, delta):
        # The native menu aligns its selected row with the closed field.
        click(key)
        x, y, w, h = app.control(PANE + key)["rect"]
        app.act("click", "w=0", f"x={x+25}", f"y={y+h/2+delta*30}", "wait=1")

    def grab(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    def page():
        app.click("tabs.plugin_tab_aisle_scene")
        app.click("aisle_scene.composer_mode")

    def pos(name, x=0.5, y=0.5):
        left, top, w, h = app.control(PANE + name)["rect"]
        return (left + w * x, top + h * y)

    def drag(start, end):
        for kind, fraction in [
            ("down", 0.0),
            ("move", 0.25),
            ("move", 0.75),
            ("move", 1.0),
            ("up", 1.0),
        ]:
            x, y = (a + (b - a) * fraction for a, b in zip(start, end))
            app.act("mouse", "w=0", "k=" + kind, f"x={x}", f"y={y}", "wait=1")

    def records():
        return [
            (p, json.loads(p.read_bytes())) for p in (root / "scenes").glob("*/*.json")
        ]

    with app.launch():
        page()
        show("header", "0 objects")
        edit("instruction", "Place one coffee table and two chairs in the room")
        click("propose")
        show("summary", "3 objects")
        assert records() == []
        grab("proposal")
        click("apply")
        show("header", "3 objects")
        click("top_view")
        choose("palette", 2)
        drag(pos("palette_tile"), pos("canvas.video_area.frame_image", 0.53, 0.52))
        show("header", "4 objects")
        show("selected_name", "Cube")
        choose("palette", 1)
        drag(pos("palette_tile"), pos("canvas.video_area.frame_image", 0.71, 0.28))
        show("header", "5 objects")
        show("selected_name", "Ottoman")

        # In plan view the 6 × 4 m fixture spans 80 frame pixels per metre.
        # Move the left chair through a real drag on its rendered collider.
        def plan(x, y):
            return pos(
                "canvas.video_area.frame_image",
                (400 + x * 80) / 800,
                (247 - y * 80) / 448,
            )

        drag(plan(-1.2, -4 / 3), plan(-0.85, 0.65))
        show("selected_name", "Chair")
        click("remove")
        show("header", "4 objects")
        click("undo")
        show("header", "5 objects")
        edit("scene_name", "Living room")
        click("save")
        show("feedback", "Scene revision saved")
        basepath, base = records()[0]
        assert base["name"] == "Living room"
        byname = {o["name"]: o for o in base["instances"]}
        assert set(byname) == {
            "Coffee table",
            "Chair",
            "Chair 2",
            "Cube",
            "Ottoman",
        }, byname
        assert len({o["id"] for o in base["instances"]}) == 5
        assert all(
            abs(a - b) < 0.012
            for a, b in zip(byname["Chair"]["position_m"], [-0.85, 0.65, 0.0])
        ), byname["Chair"]
        assert byname["Chair"]["asset"] == byname["Chair 2"]["asset"]
        assert byname["Cube"]["position_m"] != byname["Ottoman"]["position_m"]
        digest = hashlib.sha256(basepath.read_bytes()).hexdigest()
        grab("five-instances")
    with app.launch():
        page()
        show("header", "Living room")
        show("header", "5 objects")
        edit("instruction", "Move the left chair beside the ottoman")
        click("propose")
        show("summary", "5 objects")
        click("apply")
        edit("scene_name", "Living room variant")
        click("save_as")
        show("feedback", "Scene revision saved")
        variant = next(s for p, s in records() if s["id"] != base["id"])
        assert variant["name"] == "Living room variant" and variant["parent"] == dict(
            id=base["id"], revision=1
        )
        assert set(o["id"] for o in variant["instances"]) == {
            o["id"] for o in base["instances"]
        }
        moved = [
            o["name"]
            for o in variant["instances"]
            if o["position_m"] != byname[o["name"]]["position_m"]
        ]
        assert moved == ["Chair"], moved
        choose("library", -1)
        click("open_scene")
        show("header", "Living room   ·")
        assert hashlib.sha256(basepath.read_bytes()).hexdigest() == digest
    (folder / "verification.json").write_text(
        json.dumps(
            dict(
                backend=args.backend,
                base=base,
                variant=variant,
                restored_base_sha256=digest,
            ),
            indent=2,
        )
        + "\n"
    )
    print(
        f"{args.backend}: two chairs share an asset revision, five distinct instances, deletion undone, variant moved one chair, original reopened with matching SHA-256"
    )


if __name__ == "__main__":
    main()
