"""Drop JPEG and PNG photos into native Scene and reopen them after relocating the sources."""

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
PANE = "aisle_scene.project_survey."


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--studio", required=True, type=Path)
    cli.add_argument("--remote", required=True, type=Path)
    cli.add_argument("--session", required=True, type=Path)
    cli.add_argument("--backend", choices=["headless", "offscreen"], default="headless")
    cli.add_argument(
        "--assets",
        action="store_true",
        help="Also edit annotations and save physical assets",
    )
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    source = folder / "imports"
    source.mkdir()
    jpg = source / "room.jpg"
    png = source / "scene.png"
    broken = source / "damaged.jpg"
    shutil.copyfile(
        ROOT / "docs/product/aisle-studio/environment/survey-fixture.jpg", jpg
    )
    shutil.copyfile(ROOT / "docs/product/aisle-studio/workbench-design.png", png)
    broken.write_bytes(b"invalid image")
    valid = [jpg, png]
    if args.assets:
        closeup = source / "table-leg.jpg"
        shutil.copyfile(
            ROOT / "docs/product/aisle-studio/environment/survey-detail.jpg", closeup
        )
        valid.append(closeup)
    expected = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in valid}
    report = folder / "ui.json"
    env = os.environ | dict(
        MAKEPAD_HAL_BACKEND=args.backend,
        DOROBOT_UX_REPORT=str(report),
        DOROBOT_STUDIO_STATE_DIR=str(folder / "state"),
        DOROBOT_STUDIO_SOURCE_ROOT=str(ROOT),
    )

    def wait(pred):
        until = time.monotonic() + 35
        last = {}
        while time.monotonic() < until:
            if proc.poll() is not None:
                raise RuntimeError(f"Studio exited with {proc.returncode}")
            if report.exists():
                last = json.loads(report.read_bytes())
                if pred(last):
                    return last
            time.sleep(0.1)
        raise RuntimeError(
            {k: v for k, v in last.get("controls", {}).items() if k.startswith(PANE)}
        )

    def act(command, *fields):
        p = subprocess.run(
            [args.remote, "app", str(proc.pid), command, *fields],
            check=True,
            text=True,
            capture_output=True,
        )
        for line in p.stdout.splitlines():
            reply = json.loads(line)
            if "err" in reply:
                raise RuntimeError(reply)
        return p.stdout

    def target(key, command="click", *fields):
        def ready(d):
            c = d["controls"].get(key, {})
            return (
                c.get("visible") and c["rect"][2] > 0 and c.get("enabled") is not False
            )

        wait(ready)
        time.sleep(0.15)
        # Makepad reports the child's layout rectangle even when a scroll viewport clips it.
        in_form = {
            "asset_name",
            "category",
            "outline",
            "floor_ref",
            "dimension",
            "next_axis",
            "previous_axis",
            "length_value",
            "length_unit",
            "apply_length",
            "clear_marks",
            "edit_marks",
            "edit_physics",
            "asset_shape",
            "movable",
            "material",
            "friction",
            "restitution",
            "mass",
            "top_thickness",
            "leg_width",
            "graspable",
            "surface_name",
            "preview_physics",
            "rotate_geometry",
            "new_asset",
            "older_asset",
            "next_asset",
        }
        if key.startswith(PANE) and key[len(PANE) :] in in_form:
            for _ in range(12):
                controls = wait(ready)["controls"]
                x, y, w, h = controls[key]["rect"]
                fx, fy, fw, fh = controls[PANE + "form_scroll"]["rect"]
                cy = y + h / 2
                if fy + 4 <= cy <= fy + fh - 4:
                    break
                delta = max(-220, min(220, cy - (fy + fh / 2)))
                act(
                    "mouse",
                    "k=scroll",
                    "w=0",
                    f"x={fx+fw*0.7}",
                    f"y={fy+fh*0.65}",
                    "dx=0",
                    f"dy={delta}",
                    "wait=1",
                )
                time.sleep(0.16)
            else:
                raise RuntimeError(f"{key} could not be scrolled into view")
        x, y, w, h = wait(ready)["controls"][key]["rect"]
        return act(command, "w=0", f"x={x+w/2}", f"y={y+h/2}", *fields, "wait=1")

    @contextmanager
    def launch():
        nonlocal proc
        report.unlink(missing_ok=True)
        with (folder / "app.log").open("a") as log:
            proc = subprocess.Popen(
                [args.studio.resolve(), "--remote"],
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                target("tabs.plugin_tab_aisle_scene")
                target("aisle_scene.asset_mode")
                yield
            finally:
                proc.terminate()
                proc.wait(15)

    def shows(key, substring):
        return wait(lambda d: substring in d["controls"][PANE + key]["text"])

    proc = None
    with launch():
        for path in (*valid, broken):
            target(PANE + "photo_canvas", "drop", "path=" + str(path))
            shows("import_result", path.name)
        shows("file_label", valid[-1].name)
        if args.assets:
            from check_survey_assets import exercise, restored

            saved_assets = exercise(target, act, wait, shows, folder, args.backend)
        if args.backend == "offscreen":
            act("grab", "w=0", "out=" + str(folder / "imported.png"))
    source.rename(folder / "moved-originals")
    with launch():
        shows("file_label", "room.jpg")
        target(PANE + "next_photo")
        shows("file_label", "scene.png")
        target(PANE + "thumb0.select")
        shows("file_label", "room.jpg")
        if args.assets:
            restored(target, shows, folder, saved_assets)
    photos = {}
    for p in (folder / "state/aisle/survey/photos").glob("*/photo.json"):
        data = json.loads(p.read_bytes())
        original = p.parent / ("original." + data["format"])
        assert data["sha256"] == hashlib.sha256(original.read_bytes()).hexdigest()
        assert (p.parent / "thumbnail.png").is_file()
        photos[data["filename"]] = data["sha256"]
    assert photos == expected
    receipt = {"backend": args.backend, "restored_photos_sha256": photos}
    if args.assets:
        receipt["restored_revisions_sha256"] = saved_assets
        root = folder / "state/aisle/survey"
        receipt["assets"] = [
            {
                k: a[k]
                for k in ("id", "name", "revision", "dimensions", "physics", "geometry")
            }
            for key in sorted(saved_assets)
            for a in [json.loads((root / key).read_bytes())]
        ]
    (folder / "verification.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(
        f"{args.backend}: restored {len(photos)} original photos after relocation and restart"
    )
    if args.assets:
        print(
            f"Verified {len(saved_assets)} immutable revisions, source-pixel marks, mixed units, five table colliders, placement surface and graspable cube mass."
        )


if __name__ == "__main__":
    main()
