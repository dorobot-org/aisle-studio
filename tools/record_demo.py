"""Record an uncut, wall-clock 3× capture of the standalone Nano transfer."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

TASK = "Move all three floor cubes onto the coffee table in red, green, blue order."
ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--studio", type=Path, required=True)
    parser.add_argument("--remote", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--session",
        type=Path,
        required=True,
        help="Fresh directory for state and capture evidence",
    )
    args = parser.parse_args()
    session = args.session.resolve()
    session.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    output.parent.mkdir(exist_ok=True, parents=True)
    if output.exists():
        parser.error("Choose a new movie filename")
    report = session / "ui.json"
    env = os.environ | dict(
        MAKEPAD_HAL_BACKEND="offscreen",
        DOROBOT_UX_REPORT=str(report),
        DOROBOT_STUDIO_STATE_DIR=str(session / "state"),
        DOROBOT_STUDIO_SOURCE_ROOT=str(ROOT),
    )
    binary = args.studio.resolve()
    with (
        (session / "app.log").open("w") as log,
        tempfile.TemporaryDirectory(dir=session) as frame_dir,
    ):
        app = subprocess.Popen(
            [binary, "--remote"],
            cwd=ROOT,
            stdout=log,
            stderr=subprocess.STDOUT,
            env=env,
        )
        stopped = threading.Event()
        captures, problems = [], []
        folder = Path(frame_dir)

        def native(*command):
            res = subprocess.run(
                [args.remote, "app", str(app.pid), *command],
                capture_output=True,
                text=True,
                timeout=30,
                check=True,
            )
            for line in res.stdout.splitlines():
                item = json.loads(line)
                if "err" in item:
                    raise RuntimeError(item["err"])

        def read():
            return json.loads(report.read_bytes()) if report.exists() else {}

        def wait(test, seconds=45):
            end = time.monotonic() + seconds
            while time.monotonic() < end:
                if problems:
                    raise RuntimeError(problems[0])
                if app.poll() is not None:
                    raise RuntimeError("Studio closed unexpectedly")
                data = read()
                if data and test(data):
                    return data
                time.sleep(0.12)
            raise TimeoutError(read().get("state"))

        def click(key):
            def enabled(data):
                x = data["controls"][key]
                return x["visible"] and x["enabled"] is not False and x["rect"][2] > 0

            d = wait(enabled)
            x, y, w, h = d["controls"][key]["rect"]
            native("click", "w=0", f"x={x + w / 2}", f"y={y + h / 2}", "wait=1")

        def capture():
            try:
                while not stopped.is_set():
                    start = time.monotonic()
                    dest = folder / f"frame-{len(captures):06}.png"
                    native("grab", "w=0", "scale=1", "out=" + str(dest))
                    if not dest.is_file():
                        raise RuntimeError("Makepad returned no window pixels")
                    captures.append((time.monotonic(), dest))
                    stopped.wait(max(0, 0.125 - (time.monotonic() - start)))
            except Exception as error:
                problems.append(str(error))

        recorder = threading.Thread(target=capture)
        try:
            wait(lambda d: bool(d["state"]["page"]))
            time.sleep(1)
            recorder.start()
            time.sleep(2)
            click("tabs.plugin_tab_aisle_task")
            click("aisle_task.instruction")
            native(
                "key",
                "w=0",
                "c=KeyA",
                "logo=1" if sys.platform == "darwin" else "ctrl=1",
                "wait=1",
            )
            native("text", "w=0", "t=" + TASK, "wait=1")
            click("aisle_task.codex")
            d = wait(
                lambda d: not d["state"]["busy"] and bool(d["state"]["controller"]), 240
            )
            plan = d["state"]["controller"]
            assert (
                plan["provider"] == "codex" and plan["color_order"] == "red,green,blue"
            ), plan
            print("Codex prepared the three-cube controller", flush=True)
            click("tabs.plugin_tab_aisle_robot")
            time.sleep(1)
            click("aisle_robot.compact_visuals")
            time.sleep(2)
            click("tabs.plugin_tab_aisle_run")
            time.sleep(1)
            click("aisle_run.start_run")
            wait(lambda d: d["state"]["busy"])
            print("Nexus transfer started", flush=True)
            d = wait(lambda d: not d["state"]["busy"], 2700)
            result = d["state"]["result"]
            assert result["task_success"] and result["requested"] == [
                "red",
                "green",
                "blue",
            ], result
            assert all(
                result["cubes"][color]["passed"] for color in result["requested"]
            )
            click("tabs.plugin_tab_aisle_evidence")
            wait(lambda d: d["state"]["page"] == "aisle_evidence")
            time.sleep(5)
        finally:
            stopped.set()
            if recorder.is_alive():
                recorder.join(32)
            app.terminate()
            app.wait(timeout=20)
        if problems or len(captures) < 2:
            raise RuntimeError(problems or "No usable window frames")

        # Sample the actual wall clock. Slow grabs retain their last displayed
        # image for the right duration instead of speeding up a slow segment.
        import bisect

        clock = [stamp - captures[0][0] for stamp, _ in captures]
        fps = 24
        count = round(clock[-1] / 3 * fps)
        ff = subprocess.Popen(
            [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-n",
                "-f",
                "image2pipe",
                "-framerate",
                str(fps),
                "-vcodec",
                "png",
                "-i",
                "-",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "21",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                str(output),
            ],
            stdin=subprocess.PIPE,
        )
        try:
            for i in range(count):
                ix = max(0, bisect.bisect_right(clock, i * 3 / fps) - 1)
                ff.stdin.write(captures[ix][1].read_bytes())
        finally:
            ff.stdin.close()
        if ff.wait() != 0:
            raise RuntimeError("Movie encoding failed")
        meta = dict(
            app="AISLE Studio",
            backend="offscreen",
            capture="Makepad window grab",
            speed=3,
            elapsed_seconds=clock[-1],
            video_seconds=count / fps,
            original_window_frames=len(clock),
            output_frames=count,
            provider=plan["provider"],
            model=plan.get("policy_model"),
            requested=result["requested"],
            cubes=result["cubes"],
            physics_ticks=result["physics_ticks"],
            wheel_supported_ticks=result["wheel_supported_ticks"],
            rejected_motor_writes=result["rejected_motor_writes"],
            task_success=result["task_success"],
        )
        (session / "capture.json").write_text(json.dumps(meta, indent=2) + "\n")
        print(f"{output}: {count / fps:.2f}s from {clock[-1]:.2f}s", flush=True)


if __name__ == "__main__":
    main()
