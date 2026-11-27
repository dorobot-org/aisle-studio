"""Exercise the standalone G1 reference task with real Makepad input, then reopen it."""

import argparse
import hashlib
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
    parser.add_argument(
        "--installed",
        action="store_true",
        help="Exercise a relocated macOS app offline with developer resources denied",
    )
    parser.add_argument("--robot", choices=("G1", "Nano"), default="G1")
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

    command = [str(args.studio.resolve()), "--remote"]
    cwd = ROOT
    if args.installed:
        contents = args.studio.resolve().parents[1]
        release = json.loads((contents / "Resources/release.json").read_bytes())
        cwd = folder
        env = {
            k: v
            for k, v in env.items()
            if k
            in (
                "HOME",
                "TMPDIR",
                "USER",
                "LOGNAME",
                "LANG",
                "MAKEPAD_HAL_BACKEND",
                "DOROBOT_UX_REPORT",
                "DOROBOT_STUDIO_STATE_DIR",
            )
        }
        env["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
        denied = [
            ROOT,
            ROOT.parent / "AISLE-studio-runtime",
            ROOT.parent / "AISLE",
            Path("/opt/homebrew"),
            Path.home() / ".cargo",
            Path.home() / ".local/share/uv",
        ]
        rules = " ".join(
            "(subpath " + json.dumps(str(path.resolve())) + ")" for path in denied
        )
        sandbox = folder / "installed.sb"
        sandbox.write_text(
            "(version 1)\n(allow default)\n(deny network-outbound)\n"
            '(allow network-outbound (remote ip "localhost:*") (subpath "/private/tmp") (subpath "/tmp"))\n'
            + "(deny file-read* "
            + rules
            + ")\n"
            + "(deny file-write* (subpath "
            + json.dumps(str(contents))
            + "))\n"
        )
        command = ["/usr/bin/sandbox-exec", "-f", str(sandbox), *command]

    @contextmanager
    def launch():
        report.unlink(missing_ok=True)
        with (folder / "app.log").open("a") as log:
            process = subprocess.Popen(
                command,
                cwd=cwd,
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
        click(pid, "tabs.plugin_tab_aisle_robot")
        click(pid, "aisle_robot." + args.robot.lower())
        click(pid, "aisle_robot.next_scene")
        click(pid, "aisle_scene.next_task")
        click(pid, "aisle_task.reference")
        wait(
            lambda d: d["state"]["controller"].get("robot") == args.robot
            if isinstance(d["state"]["controller"], dict)
            else False
        )
        click(pid, "tabs.plugin_tab_aisle_scene")
        wait(lambda d: d["state"]["page"] == "aisle_scene")
        click(pid, "tabs.plugin_tab_aisle_run")
        click(pid, "aisle_run.start_run")
        row = wait(
            lambda d: (d["state"]["result"] or {}).get("robot") == args.robot
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
        restored = wait(
            lambda d: d["state"]["page"] == "aisle_evidence"
            and (d["state"]["result"] or {}).get("task_success")
        )
        assert (
            restored["selected"] == selected
            and restored["result"]["controller_hash"] == result["controller_hash"]
        )
    if args.installed:
        source = json.loads(
            next((folder / "state/aisle").glob("*/source.json")).read_bytes()
        )
        adapter = contents / "Resources/AISLE/src/aisle/sim/nexus_backend.py"
        assert source["aisle_revision"] == release["aisle_revision"]
        assert (
            source["adapter_hash"] == hashlib.sha256(adapter.read_bytes()).hexdigest()
        )
        assert source["backend"] == "metal" and source["nexus"] == release["nexus"]
        print(
            f"Verified packaged AISLE {source['aisle_revision']} and Nexus {source['nexus']} on Metal."
        )
    print(
        f"{args.robot} reference, five pages, scored simulation, episode export, replay and restart passed."
    )


if __name__ == "__main__":
    main()
