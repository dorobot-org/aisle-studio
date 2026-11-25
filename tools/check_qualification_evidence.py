"""Inspect real per-case measurements in the headless or offscreen qualification page."""

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
from PIL import Image

from studio_native import NativeStudio


def read(path):
    return json.loads(path.read_bytes())


def check_exposure(app, key, file, screenshot=None):
    tick = int(file.stem)
    state = app.wait(
        lambda st: f"tick {tick} ·" in st["controls"][key + "exposure_caption"]["text"],
        f"exposure {tick}",
    )
    assert state["controls"][key + "exposure_rgb"]["visible"]
    with np.load(file, allow_pickle=False) as packet:
        expected = np.concatenate(
            [packet[k] for k in ("head", "left_wrist", "right_wrist")], axis=1
        )
    checksum = hashlib.sha256(expected.tobytes()).hexdigest()
    assert checksum[:12] in state["controls"][key + "exposure_caption"]["text"]
    proof = dict(tick=tick, rgb_sha256=checksum)
    if screenshot:
        app.scroll_to(key + "exposure_rgb", key + "panel")
        x, y, w, h = app.control(key + "exposure_rgb")["rect"]
        app.act("grab", "w=0", "out=" + str(screenshot))
        raw = Image.open(screenshot).convert("RGB")
        frame = Image.fromarray(expected)
        scale = min(w / frame.width, h / frame.height)
        rw, rh = frame.width * scale, frame.height * scale
        x += (w - rw) / 2
        y += (h - rh) / 2
        observed = raw.crop((round(x), round(y), round(x + rw), round(y + rh)))

        def regions(image):
            width, height = image.size
            for j in range(3):
                # Average the interior of each camera to tolerate subpixel rasterization.
                rect = (
                    round((j + 0.06) * width / 3),
                    round(0.06 * height),
                    round((j + 0.94) * width / 3),
                    round(0.94 * height),
                )
                yield np.asarray(
                    image.crop(rect).resize((32, 24), Image.Resampling.BOX), dtype=float
                )

        errors = [
            float(np.abs(a - b).mean())
            for a, b in zip(regions(observed), regions(frame), strict=True)
        ]
        assert max(errors) < 22, (
            f"Rendered head/left/right RGB differ from the exposure: {errors}"
        )
        proof["mean_pixel_error_per_camera"] = [round(e, 2) for e in errors]
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("studio", "remote", "session", "source"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument(
        "--backend", choices=["offscreen", "headless"], default="headless"
    )
    args = parser.parse_args()
    folder = args.session.resolve()
    state = folder / "state/aisle"
    source = args.source.resolve()
    cohorts = state / "qualifications"
    cohorts.mkdir(parents=True, exist_ok=False)
    (cohorts / source.name).symlink_to(source, target_is_directory=True)
    report_bytes = (source / "verification.json").read_bytes()
    report = json.loads(report_bytes)
    rows = read(source / "qualification-rows.json")
    robot = report["robot"]
    key = "aisle_evidence.qa_"
    app = NativeStudio(args, folder)
    checks = {}

    def wait_text(name, expected):
        return app.wait(
            lambda s: expected in s["controls"].get(key + name, {}).get("text", ""),
            expected,
        )

    def select(index):
        app.scroll_to(key + "case_selector", key + "panel")
        app.click(key + "case_selector")
        for c in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + c, "wait=1")
        wait_text("case_summary", f"Case {index} ·")

    with app.launch():
        app.click("aisle_task." + robot.lower())
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click(key + "mode")
        app.click(key + "inspect")
        wait_text("case_summary", "Case 0 ·")
        if robot == "G1":
            corners = read(source / "suite.json")["boundary_cases"]
            wait_text("case_summary", "Boundary impact")
            select(corners)
            row = rows[corners]
            wait_text("case_summary", f"{row['wrist_error_mm']:.2f} mm")
            wait_text("case_summary", "Interior impact")
            state_report = wait_text("case_coverage", "boundary impacts passed")
            assert not state_report["controls"][key + "exposure_rgb"]["visible"]
            checks = dict(
                case=row["index"],
                support_steps=row["loaded_steps"],
                wrist_error_mm=row["wrist_error_mm"],
            )
        else:
            wait_text("case_coverage", "95% Wilson interval")
            index = next(
                (
                    r["index"]
                    for r in rows
                    if r["artifacts"].get("release_exposures", 0) >= 2
                ),
                0,
            )
            if index:
                select(index)
            case = source / f"case-{index:03}"
            files = sorted((case / "release-observations").glob("*.npz"))
            wait_text("case_summary", f"Layout seed {rows[index]['opening']['seed']}")
            checks = dict(case=index, source_release_exposures=len(files), frames=[])
            if len(files) >= 2:
                for j, f in enumerate(files[:2]):
                    if j:
                        app.scroll_to(key + "next_exposure", key + "panel")
                        app.click(key + "next_exposure")
                    wait_text("exposure_count", f"Exposure {j + 1} of {len(files)}")
                    raster = (
                        folder / f"release-{j}.png"
                        if args.backend == "offscreen"
                        else None
                    )
                    checks["frames"].append(check_exposure(app, key, f, raster))
                assert checks["frames"][0]["tick"] != checks["frames"][1]["tick"]
                app.scroll_to(key + "previous_exposure", key + "panel")
                app.click(key + "previous_exposure")
                wait_text("exposure_caption", f"tick {int(files[0].stem)} ·")
                app.scroll_to(key + "exposure_rgb", key + "panel")
            else:
                view = wait_text("exposure_caption", "No recorded release exposures")
                assert not view["controls"][key + "exposure_rgb"]["visible"]
        checks["coverage"] = app.control(key + "case_coverage")["text"]
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / "case-evidence.png"))
        app.scroll_to(key + "inspect", key + "panel")
        app.click(key + "inspect")
        app.wait(
            lambda s: not s["controls"][key + "inspector"]["visible"],
            "closed case evidence",
        )
    assert (source / "verification.json").read_bytes() == report_bytes
    proof = dict(
        robot=robot,
        registration=source.name,
        backend=args.backend,
        verdict_sha256=hashlib.sha256(report_bytes).hexdigest(),
        **checks,
    )
    (folder / "verification.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps(proof))


if __name__ == "__main__":
    main()
