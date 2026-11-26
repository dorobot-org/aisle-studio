"""Exercise the native navigation from a fresh project at both supported window sizes."""

import argparse
import json
import time
from pathlib import Path
from zipfile import ZipFile

from studio_native import NativeStudio


PAGES = ("robot", "scene", "task", "run", "evidence")
SIZES = ((1024, 768), (1440, 900))


def tab(page):
    return "tabs.plugin_tab_aisle_" + page


def within(rect, width, height):
    x, y, w, h = rect
    return (
        x >= 0
        and y >= 0
        and w > 0
        and h > 0
        and x + w <= width + 1
        and y + h <= height + 1
    )


def wait_page(app, page):
    return app.wait(lambda s: s["state"]["page"] == "aisle_" + page, page)


def check_mode(app, key, heading, expected):
    app.click(key)
    app.wait(lambda s: s["controls"][heading]["text"] == expected, expected)


def capture(app, folder, page, width, height, rects, receipt):
    from PIL import Image

    image = folder / f"{page}-{width}.png"
    deadline = time.monotonic() + 35
    while True:
        app.act("grab", "w=0", "out=" + str(image))
        if page not in ("robot", "task"):
            break
        # Both fresh previews use the Nano URDF. Its orange arms appear only
        # after the asynchronous mesh load and the first paint of the model.
        x, y, w, h = app.control(f"aisle_{page}.row.model_card.robot")["rect"]
        with Image.open(image) as raster:
            crop = raster.crop((round(x), round(y), round(x + w), round(y + h)))
            thumb = crop.convert("RGB").resize((256, 192))
            pixels = thumb.load()
            arm_pixels = sum(
                r > 130 and r - g > 45 and g - b > 12
                for r, g, b in (pixels[x, y] for y in range(thumb.height) for x in range(thumb.width))
            )
        if arm_pixels >= 35:
            receipt["nano_arm_pixels_in_thumbnail"] = arm_pixels
            break
        if time.monotonic() >= deadline:
            raise RuntimeError(f"{page} Nano mesh did not render at {width} × {height}")
        time.sleep(0.2)
    with Image.open(image) as raster:
        assert raster.size == (width, height)
        rgb = raster.convert("RGB")
        for name, (x, y, w, h) in rects.items():
            # All four tabs have different symbols. Sample the face before the symbol.
            r, g, b = rgb.getpixel((round(x + 8), round(y + h / 2)))
            assert (b - r > 15) == (name == page), (name, page, r, g, b)
        bright_cards = {
            "robot": "aisle_robot.inspector",
            "run": "aisle_run.empty_run",
        }
        if page in bright_cards:
            x, y, w, _ = app.control(bright_cards[page])["rect"]
            paper = rgb.getpixel((round(x + w - 22), round(y + 20)))
            assert min(paper) >= 235, (page, "card background", paper)
            heading = rgb.crop(
                (round(x + 12), round(y + 6), round(x + w - 20), round(y + 45))
            ).convert("L")
            # Dark glyph interiors against the light surface, excluding borders.
            assert sum(heading.histogram()[:90]) >= 35, (page, "unreadable heading")
            receipt["card_background_rgb"] = paper
    return image.name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("studio", "remote", "session"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--backend", choices=("offscreen", "headless"), required=True)
    args = parser.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    app = NativeStudio(args, folder)
    results = []
    with app.launch():
        wait_page(app, "robot")
        app.control("aisle_robot.nano")
        app.click(tab("robot"))
        for width, height in SIZES:
            app.act("resize", "window=0", f"w={width}", f"h={height}")

            def centered(s):
                first = s["controls"][tab("robot")]["rect"]
                last = s["controls"][tab("evidence")]["rect"]
                return abs(first[0] + last[0] + last[2] - width) < 4

            app.wait(centered, "tab bar resize")
            for page in PAGES:
                app.click(tab(page))
                wait_page(app, page)
                time.sleep(0.2)
                rects = {name: app.control(tab(name))["rect"] for name in PAGES}
                for i, name in enumerate(PAGES):
                    x, y, w, h = rects[name]
                    assert within(rects[name], width, height) and min(w, h) >= 44
                    if i:
                        px, py, pw, ph = rects[PAGES[i - 1]]
                        assert px + pw <= x + 0.1 and abs(py - y) < 1
                first, last = rects["robot"], rects["evidence"]
                assert abs((first[0] + last[0] + last[2]) / 2 - width / 2) < 2
                if page == "task":
                    for name in (
                        "reference",
                        "codex",
                        "octos",
                        "onboard",
                        "skill_mode",
                        "select_scene",
                    ):
                        r = app.control("aisle_task." + name)["rect"]
                        assert within(r, width, height) and min(r[2:]) >= 44, (name, r)
                if page == "robot":
                    for name in ("nano", "g1", "next_scene", "joint_mode", "cameras_mode", "hardware_mode"):
                        hit = app.control("aisle_robot." + name)["rect"]
                        assert within(hit, width, height) and min(hit[2:]) >= 44, (name, hit)
                if page == "scene":
                    assert app.control("aisle_scene.scene_heading")["text"] == "Scene editor"
                    app.control("aisle_scene.composer.canvas")
                receipt = dict(size=[width, height], page=page, tabs=rects)
                if args.backend == "offscreen":
                    receipt["image"] = capture(
                        app, folder, page, width, height, rects, receipt
                    )
                results.append(receipt)
        for page, button in [("run", "prepare_run"), ("evidence", "prepare_evidence")]:
            app.click(tab(page))
            app.click(f"aisle_{page}.{button}")
            wait_page(app, "task")
        for n, page in enumerate(PAGES, 1):
            app.act("key", "w=0", "k=press", "c=Key" + str(n), "logo=1", "wait=1")
            wait_page(app, page)
        for page, choices in [
            (
                "run",
                [
                    ("experiment_mode", "Repeat experiment"),
                    ("workers", "Workers"),
                    ("local_run", "Live simulation"),
                ],
            ),
            (
                "evidence",
                [
                    ("qa_mode", "Skill qualification"),
                    ("ev_mode", "Experiment trials"),
                    ("evaluation_mode", "Batch evaluation"),
                    ("device_mode", "Preflight results"),
                    ("trial_mode", "Run evidence"),
                ],
            ),
        ]:
            app.click(tab(page))
            heading = (
                "aisle_" + page + (".run_heading" if page == "run" else ".ev_heading")
            )
            for name, label in choices:
                key = "aisle_" + page + "." + name
                check_mode(app, key, heading, label)
                check_mode(app, key, heading, label)
        app.click(tab("robot"))
        app.click("aisle_robot.next_scene")
        wait_page(app, "scene")
        app.click("aisle_scene.next_task")
        wait_page(app, "task")
        app.click(tab("run"))
        app.click("aisle_run.check_scene")
        wait_page(app, "scene")
        app.click(tab("run"))
        app.click("aisle_run.check_robot")
        wait_page(app, "robot")
        app.click("aisle_robot.g1")
        app.click("aisle_robot.joint_mode")
        app.wait(lambda s: " of " in s["controls"]["aisle_robot.joint_count"]["text"], "G1 URDF joints")
        app.click("aisle_robot.hardware_mode")
        assert "simulation" in app.control("aisle_robot.device_title")["text"]
        app.click("aisle_robot.robot_mode")
        app.click(tab("task"))
        app.wait(
            lambda s: not s["controls"]["aisle_task.onboard"]["visible"], "G1 profile"
        )
        app.click(tab("robot"))
        app.click("aisle_robot.nano")
        app.click("aisle_robot.cameras_mode")
        fixture = Path(__file__).resolve().parents[1] / "docs/product/aisle-studio/validation/room-survey/nano-camera-survey.zip"
        with ZipFile(fixture) as archive:
            archive.extractall(folder / "fixture")
        app.click("aisle_robot.calibration_path")
        app.act("text", "w=0", "t=" + str(folder / "fixture/survey.json"), "wait=1")
        app.click("aisle_robot.import_calibration")
        app.wait(lambda s: "5 check points" in s["controls"]["aisle_robot.calibration_detail"]["text"], "measured Nano calibration")
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / "nano-calibration.png"))
        app.click(tab("task"))
        app.click("aisle_task.onboard")
        app.wait(
            lambda s: "on" == s["controls"]["aisle_task.onboard"]["text"].split()[-1],
            "onboard RGB enabled",
        )
        app.click("aisle_task.select_scene")
        app.click(tab("scene"))
        app.click(tab("task"))
        app.wait(
            lambda s: s["controls"]["aisle_task.task_heading"]["text"]
            == "Bind scene task",
            "task position restored",
        )
        app.click("aisle_task.select_scene")
        app.click("aisle_task.onboard")
        app.wait(
            lambda s: s["controls"]["aisle_task.onboard"]["text"].endswith("off"),
            "sensor choice persisted",
        )
        app.click("aisle_task.reference")
        app.wait(
            lambda s: bool(s["state"]["controller"]) and not s["state"]["busy"],
            "prepared reference",
        )
        app.click("aisle_task.next_run")
        wait_page(app, "run")
        app.click(tab("evidence"))
        assert app.control("aisle_evidence.prepare_evidence")["text"] == "Open Run"
        app.click("aisle_evidence.prepare_evidence")
        wait_page(app, "run")
        app.control("aisle_run.start_run")
        controller = app.wait(lambda s: bool(s["state"]["controller"]))["state"]["controller"]
        assert set(controller["robot_camera_profile"]["cameras"]) == {"head", "left_wrist", "right_wrist"}
    with app.launch():
        wait_page(app, "run")
        app.control("aisle_run.start_run")
        app.click(tab("robot"))
        app.click("aisle_robot.cameras_mode")
        assert "5 check points" in app.control("aisle_robot.calibration_detail")["text"]
        app.click("aisle_robot.clear_calibration")
        app.wait(lambda s: "URDF camera defaults" in s["controls"]["aisle_robot.calibration_detail"]["text"], "clear camera override")
        assert controller == app.wait(lambda s: bool(s["state"]["controller"]))["state"]["controller"]
        # Register the room camera and the robot start in Scene, then save and reopen.
        app.click(tab("scene"))
        pre = "aisle_scene.composer."
        app.click(pre + "registration_mode")
        for field, value in [("robot_start", "0.1 -0.2 15"), ("camera_poses", "ceiling 0 1 2.4 0 90 0")]:
            app.click(pre + field)
            app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
            app.act("text", "w=0", "t=" + value, "wait=1")
        app.scroll_to(pre + "apply_registration", pre + "inspector", whole=True)
        app.click(pre + "apply_registration")
        app.wait(lambda s: "Registration applied" in s["controls"][pre + "feedback"]["text"], "apply room registration")
        app.click(pre + "save")
        app.control("aisle_scene.create_bound_task")
        app.scroll_to(pre + "discard", pre + "inspector", whole=True)
        app.click(pre + "discard")
        app.wait(lambda s: "ceiling" in s["controls"][pre + "camera_poses"]["text"], "reopen room camera")
        files = (folder / "state/aisle/survey/scenes").rglob("*.json")
        assert any(
            isinstance(doc, dict) and doc.get("registration", {}).get("robot_xy_yaw") == [0.1, -0.2, 15]
            and doc["registration"]["fixed_cameras"] == {"ceiling": [0, 1, 2.4, 0, 90, 0]}
            for doc in (json.loads(f.read_bytes()) for f in files)
        )
        if args.backend == "offscreen":
            app.scroll_to(pre + "robot_start", pre + "inspector", whole=True)
            app.act("grab", "w=0", "out=" + str(folder / "room-registration.png"))
    marker = folder / "state/aisle/last-page.json"
    marker.write_text('"removed-destination"')
    with app.launch():
        wait_page(app, "robot")
    proof = dict(
        backend=args.backend,
        sizes=SIZES,
        pages=PAGES,
        command_shortcuts=5,
        first_page="robot",
        resumed_page="run",
        unknown_saved_tab_falls_back_to="robot",
        saved_room_registration=True,
        g1_joint_inspection=True,
        measured_camera_mounts=["head", "left_wrist", "right_wrist"],
        calibration_pinned_in_controller=True,
        stable_mode_labels=True,
        empty_project_routes=["Task", "Run after preparation"],
        task_position_restored=True,
        captures=results,
    )
    (folder / "verification.json").write_text(json.dumps(proof, indent=2) + "\n")


if __name__ == "__main__":
    main()
