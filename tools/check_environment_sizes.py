"""Capture the populated environment workflows at both specified native window sizes."""

import argparse
import json
from pathlib import Path
import shutil
import struct

from studio_native import NativeStudio


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for name in (
        "studio",
        "remote",
        "survey-session",
        "experiment-session",
        "evidence-session",
        "output",
    ):
        cli.add_argument("--" + name, type=Path, required=True)
    args = cli.parse_args()
    args.backend = "offscreen"
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    pages = [
        (
            "survey",
            args.survey_session,
            [
                "tabs.plugin_tab_aisle_scene",
                "aisle_scene.asset_mode",
                "aisle_scene.project_survey.edit_assets",
            ],
            [
                "aisle_scene.project_survey.save_asset",
                "aisle_scene.project_survey.photo_canvas",
            ],
        ),
        (
            "scene",
            args.experiment_session,
            ["tabs.plugin_tab_aisle_scene", "aisle_scene.composer_mode"],
            ["aisle_scene.composer.save", "aisle_scene.composer.canvas"],
        ),
        (
            "task",
            args.experiment_session,
            ["tabs.plugin_tab_aisle_task", "aisle_task.select_scene"],
            ["aisle_task.project_task.activate", "aisle_task.project_task.viewport"],
        ),
        (
            "run",
            args.experiment_session,
            ["tabs.plugin_tab_aisle_run", "aisle_run.experiment_mode"],
            [
                "aisle_run.experiment_pane.exp_save",
                "aisle_run.experiment_pane.exp_start",
                "aisle_run.experiment_pane.exp_next",
            ],
        ),
        (
            "evidence",
            args.evidence_session,
            ["tabs.plugin_tab_aisle_evidence", "aisle_evidence.ev_mode"],
            [
                "aisle_evidence.ev_catalog.ev_play",
                "aisle_evidence.ev_catalog.ev_image",
                "aisle_evidence.ev_catalog.ev_search",
            ],
        ),
    ]
    report = []
    for page, source, buttons, controls in pages:
        session = out / ("session-" + page)
        session.mkdir()
        # Copy small project settings. Read-only evidence and survey assets can be shared.
        (session / "state/aisle").mkdir(parents=True)
        for item in (source / "state/aisle").iterdir():
            target = session / "state/aisle" / item.name
            if item.is_dir():
                target.symlink_to(item.resolve(), target_is_directory=True)
            elif item.is_file():
                shutil.copy2(item, target)
        app = NativeStudio(args, session)
        with app.launch():
            for key in buttons:
                app.click(key)
            for w, h in [(1024, 768), (1440, 900)]:
                app.act("resize", "window=0", f"w={w}", f"h={h}")
                path = out / f"{page}-{w}.png"
                app.act("grab", "w=0", "out=" + str(path))
                actual = struct.unpack(">II", path.read_bytes()[16:24])
                assert actual == (w, h), (page, actual, w, h)
                rects = {k: app.control(k)["rect"] for k in controls}
                for k, (x, y, width, height) in rects.items():
                    if not (
                        x >= -1
                        and y >= -1
                        and x + width <= w + 1
                        and y + height <= h + 1
                    ):
                        raise ValueError(
                            f"{page} {w}×{h}: {k} outside viewport at {rects[k]}"
                        )
                report.append(
                    dict(page=page, width=w, height=h, controls=rects, image=path.name)
                )
    (out / "window-sizes.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Verified {len(report)} native window captures")


if __name__ == "__main__":
    main()
