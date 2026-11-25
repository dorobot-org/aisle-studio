"""Run Nano gamepad teleoperation through Dora, with a local Studio preview."""

import argparse
import importlib.metadata
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import time
import yaml


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aisle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--socket", type=Path, default=Path("/tmp/nano-gamepad.sock"))
    parser.add_argument("--owner-pid", type=int)
    parser.add_argument(
        "--model",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf",
    )
    args = parser.parse_args()
    if args.output.exists() or args.socket.exists():
        parser.error("Use a new output directory and an unused socket")
    cli = shutil.which("dora")
    if (
        not cli
        or subprocess.check_output([cli, "--version"], text=True).strip()
        != "dora-cli 1.0.1"
        or importlib.metadata.version("dora-rs") != "1.0.1"
    ):
        parser.error("Dora CLI and Python API must be 1.0.1")
    root = args.output.resolve()
    root.mkdir(parents=True)
    source = root / "node-source"
    source.mkdir()
    folder = Path(__file__).resolve().parent
    for name in (
        "nano_teleop.py",
        "nano_teleop_node.py",
        "nano_teleop_graph.yaml",
        "nano_living.py",
        "nano_living_node.py",
        "nano_living_stream.py",
        "nano_living_record.py",
        "nano_cameras.py",
        "nano_workspace.py",
        "nano_visuals.py",
        "nano_session_record.py",
    ):
        shutil.copy2(folder / name, source / name)
    config = {
        k: str(v.resolve()) if isinstance(v, Path) else v for k, v in vars(args).items()
    }
    config.update(run_id=root.name, wait_viewer=False)
    (root / "config.json").write_text(json.dumps(config, indent=2))
    graph = yaml.safe_load((source / "nano_teleop_graph.yaml").read_text())
    for node in graph["nodes"]:
        node["path"] = sys.executable
        node["args"] = shlex.join(
            ["-u", str(source / "nano_teleop_node.py"), node["id"]]
        )
        node["env"] = dict(
            NANO_LIVING_CONFIG=str(root / "config.json"),
            PYTHONPATH=os.pathsep.join(
                [str(source), str(args.aisle.resolve() / "src")]
            ),
            PYTHONUNBUFFERED="1",
            SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS="1",
        )
    (root / "dataflow.yaml").write_text(yaml.safe_dump(graph, sort_keys=False))
    with (root / "dora.log").open("w") as log:
        process = subprocess.Popen(
            [cli, "run", str(root / "dataflow.yaml"), "--log-format", "json"],
            cwd=root,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )

        def terminate(_signum, _frame):
            raise KeyboardInterrupt

        signal.signal(signal.SIGTERM, terminate)
        try:
            while process.poll() is None:
                if (root / "stop").exists():
                    break
                if args.owner_pid:
                    try:
                        os.kill(args.owner_pid, 0)
                    except ProcessLookupError:
                        break
                failures = list(root.glob("failure-*.json"))
                if failures:
                    raise RuntimeError(failures[0].read_text())
                time.sleep(0.2)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        if process.returncode and not (root / "stop").exists():
            raise RuntimeError(f"Dora teleoperation stopped; see {root}/dora.log")


if __name__ == "__main__":
    main()
