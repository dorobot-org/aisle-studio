"""Launch an isolated native Studio and drive its Makepad instrumentation."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


class NativeStudio:
    def __init__(self, args, folder):
        self.args = args
        self.folder = folder
        self.report = folder / "ui.json"
        self.proc = None

    @contextmanager
    def launch(self):
        if self.proc is not None and self.proc.poll() is None:
            raise RuntimeError(
                "The prior Studio instance is still running in this test session"
            )
        self.report.unlink(missing_ok=True)
        env = os.environ | dict(
            MAKEPAD_HAL_BACKEND=self.args.backend,
            DOROBOT_UX_REPORT=str(self.report),
            DOROBOT_STUDIO_STATE_DIR=str(self.folder / "state"),
            DOROBOT_STUDIO_SOURCE_ROOT=str(ROOT),
        )
        with (self.folder / "app.log").open("a") as log:
            self.proc = subprocess.Popen(
                [self.args.studio.resolve(), "--remote"],
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                yield
            finally:
                self.proc.terminate()
                self.proc.wait(15)

    def wait(self, predicate, label="UI update", timeout=35):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if self.proc.poll() is not None:
                raise RuntimeError(f"Studio exited with {self.proc.returncode}")
            if self.report.exists():
                state = json.loads(self.report.read_bytes())
                if predicate(state):
                    return state
            time.sleep(0.1)
        raise RuntimeError(f"Timed out waiting for {label}. See {self.report}")

    def act(self, command, *fields):
        result = subprocess.run(
            [self.args.remote, "app", str(self.proc.pid), command, *fields],
            text=True,
            capture_output=True,
            check=True,
        )
        for line in result.stdout.splitlines():
            reply = json.loads(line)
            if "err" in reply:
                raise RuntimeError(reply)
        return result.stdout

    def control(self, key):
        def ready(snapshot):
            c = snapshot["controls"].get(key, {})
            return (
                c.get("visible") and c["rect"][2] > 0 and c.get("enabled") is not False
            )

        return self.wait(ready, key)["controls"][key]

    def scroll_to(self, key, viewport, *, whole=False):
        for _ in range(16):
            x, y, w, h = self.control(key)["rect"]
            fx, fy, fw, fh = self.control(viewport)["rect"]
            cy = y + h / 2
            top, bottom = (y, y + h) if whole else (cy, cy)
            if fy + 4 <= top and bottom <= fy + fh - 4:
                return
            delta = max(-210, min(210, cy - (fy + fh / 2)))
            self.act(
                "mouse",
                "k=scroll",
                "w=0",
                f"x={fx + fw * 0.7}",
                f"y={fy + fh * 0.65}",
                "dx=0",
                f"dy={delta}",
                "wait=1",
            )
            time.sleep(0.16)
        raise RuntimeError(f"{key} is clipped by {viewport}")

    def click(self, key, command="click", *fields):
        self.control(key)
        time.sleep(0.15)
        x, y, w, h = self.control(key)["rect"]
        return self.act(
            command, "w=0", f"x={x + w / 2}", f"y={y + h / 2}", *fields, "wait=1"
        )
