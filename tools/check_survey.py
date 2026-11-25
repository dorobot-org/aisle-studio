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
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    source = folder / "imports"
    source.mkdir()
    jpg = source / "robot.jpg"
    png = source / "scene.png"
    broken = source / "damaged.jpg"
    shutil.copyfile(ROOT / "media/nano-three-cubes.jpg", jpg)
    shutil.copyfile(ROOT / "docs/product/aisle-studio/workbench-design.png", png)
    broken.write_bytes(b"invalid image")
    expected = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (jpg, png)}
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
        for path in (jpg, png, broken):
            target(PANE + "photo_canvas", "drop", "path=" + str(path))
            shows("import_result", path.name)
        shows("file_label", "scene.png")
        if args.backend == "offscreen":
            act("grab", "w=0", "out=" + str(folder / "imported.png"))
    source.rename(folder / "moved-originals")
    with launch():
        shows("file_label", "robot.jpg")
        target(PANE + "next_photo")
        shows("file_label", "scene.png")
        target(PANE + "thumb0.select")
        shows("file_label", "robot.jpg")
    photos = {}
    for p in (folder / "state/aisle/survey/photos").glob("*/photo.json"):
        data = json.loads(p.read_bytes())
        original = p.parent / ("original." + data["format"])
        assert data["sha256"] == hashlib.sha256(original.read_bytes()).hexdigest()
        assert (p.parent / "thumbnail.png").is_file()
        photos[data["filename"]] = data["sha256"]
    assert photos == expected
    print(
        f"{args.backend}: JPEG and PNG accepted, per-file error shown, both originals verified after relocation and restart"
    )


if __name__ == "__main__":
    main()
