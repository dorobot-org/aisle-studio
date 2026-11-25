"""Exercise the standalone G1 reference task with real Makepad input, then reopen it."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--studio", type=Path, required=True)
    parser.add_argument("--remote", type=Path, required=True)
    parser.add_argument("--session", required=True, type=Path)
    args = parser.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    report = folder / "ui.json"
    env = os.environ | dict(
        MAKEPAD_HAL_BACKEND="headless",
        DOROBOT_UX_REPORT=str(report),
        DOROBOT_STUDIO_STATE_DIR=str(folder / "state"),
        DOROBOT_STUDIO_SOURCE_ROOT=str(ROOT),
    )

    @contextmanager
    def launch():
        report.unlink(missing_ok=True)
        with (folder / "app.log").open("a") as log:
            process = subprocess.Popen(
                [args.studio.resolve(), "--remote"],
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                wait(lambda d: d["state"]["app"] == "AISLE Studio")
                yield process.pid
            finally:
                process.terminate()
                process.wait(timeout=15)

    def wait(check, timeout=120):
        until = time.monotonic() + timeout
        data = {}
        while time.monotonic() < until:
            if report.exists():
                data = json.loads(report.read_bytes())
                if check(data):
                    return data["state"]
            time.sleep(0.12)
        raise RuntimeError(data.get("state"))

    def click(pid, key):
        wait(
            lambda d: d["controls"][key]["visible"]
            and d["controls"][key]["rect"][2] > 0
            and d["controls"][key]["enabled"] is not False
        )
        time.sleep(0.15)
        x, y, w, h = json.loads(report.read_bytes())["controls"][key]["rect"]
        command = [
            args.remote,
            "app",
            str(pid),
            "click",
            "w=0",
            f"x={x + w / 2}",
            f"y={y + h / 2}",
            "wait=1",
        ]
        reply = subprocess.run(command, check=True, capture_output=True, text=True)
        for line in reply.stdout.splitlines():
            if "err" in json.loads(line):
                raise RuntimeError(line)

    with launch() as pid:
        click(pid, "aisle_task.g1")
        click(pid, "aisle_task.reference")
        wait(
            lambda d: d["state"]["controller"].get("robot") == "G1"
            if isinstance(d["state"]["controller"], dict)
            else False
        )
        click(pid, "tabs.plugin_tab_aisle_scene")
        wait(lambda d: d["state"]["page"] == "aisle_scene")
        click(pid, "tabs.plugin_tab_aisle_run")
        click(pid, "aisle_run.start_run")
        row = wait(
            lambda d: (d["state"]["result"] or {}).get("robot") == "G1"
            and not d["state"]["busy"]
        )
        selected = row["selected"]
        result = row["result"]
        assert result["task_success"] and result["within_limits"], result
        click(pid, "tabs.plugin_tab_aisle_evidence")
        click(pid, "aisle_evidence.export")
        wait(
            lambda d: not d["state"]["busy"]
            and d["controls"]["aisle_evidence.replay"]["enabled"] is True
        )
        click(pid, "aisle_evidence.replay")
        wait(lambda d: d["state"]["replay"])
        click(pid, "back")
        wait(
            lambda d: d["state"]["page"] == "aisle_evidence"
            and not d["state"]["replay"]
        )
    with launch() as pid:
        click(pid, "tabs.plugin_tab_aisle_evidence")
        restored = wait(
            lambda d: d["state"]["page"] == "aisle_evidence"
            and (d["state"]["result"] or {}).get("task_success")
        )
        assert (
            restored["selected"] == selected
            and restored["result"]["controller_hash"] == result["controller_hash"]
        )
    print(
        "G1 reference, four pages, scored simulation, episode export, replay and restart passed."
    )


if __name__ == "__main__":
    main()
