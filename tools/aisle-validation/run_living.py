"""Launch the five-process Dora living-room simulation with a local Studio feed."""

from __future__ import annotations
import argparse
import hashlib
import importlib.metadata
import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path
import yaml


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--aisle", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument(
        "--model",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf",
    )
    p.add_argument("--socket", type=Path, required=True)
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--width", type=int, default=640)
    p.add_argument("--height", type=int, default=480)
    p.add_argument("--max-frames", type=int, help="Frame limit; 0 means no limit")
    p.add_argument(
        "--timeout", type=float, help="Wall timeout in seconds; 0 means no timeout"
    )
    p.add_argument(
        "--preview-only",
        action="store_true",
        help="Stream without saving camera recordings",
    )
    p.add_argument(
        "--continuous",
        action="store_true",
        help="Keep navigating back and forth; requires --preview-only",
    )
    p.add_argument("--no-wait-viewer", action="store_true")
    p.add_argument("--unpaced", action="store_true")
    p.add_argument(
        "--goals", type=json.loads, help="Optional explicit list of [x,y] goals"
    )
    args = p.parse_args()
    if args.continuous and not args.preview_only:
        p.error(
            "--continuous requires --preview-only; finite runs capture full recordings"
        )
    if args.max_frames is None:
        args.max_frames = 0 if args.continuous else 1200
    if args.timeout is None:
        args.timeout = 0 if args.continuous else 600
    if args.max_frames < 0 or args.timeout < 0:
        p.error("Frame limit and timeout cannot be negative")
    out = args.output.resolve()
    folder = Path(__file__).resolve().parent
    if out.exists():
        p.error("Use a new output directory")
    if args.width < 64 or args.height < 64 or args.width > 1920 or args.height > 1080:
        p.error("Unsupported camera resolution")
    cli = shutil.which("dora")
    if (
        cli is None
        or subprocess.check_output([cli, "--version"], text=True).strip()
        != "dora-cli 1.0.1"
        or importlib.metadata.version("dora-rs") != "1.0.1"
    ):
        p.error("Dora CLI and Python API must be 1.0.1")
    out.mkdir(parents=True)
    snapshot = out / "node-source"
    snapshot.mkdir()
    for name in [
        "nano_living.py",
        "nano_living_node.py",
        "nano_living_record.py",
        "nano_living_stream.py",
        "nano_living_graph.yaml",
        "nano_cameras.py",
        "nano_visuals.py",
        "nano_workspace.py",
    ]:
        shutil.copy2(folder / name, snapshot / name)
    config = {
        k: str(v.resolve()) if isinstance(v, Path) else v for k, v in vars(args).items()
    }
    config.update(
        run_id=out.name,
        wait_viewer=not args.no_wait_viewer,
        realtime=not args.unpaced,
        sources={
            f.name: hashlib.sha256(f.read_bytes()).hexdigest()
            for f in snapshot.iterdir()
        },
    )
    (out / "config.json").write_text(json.dumps(config, indent=2))
    graph = yaml.safe_load((snapshot / "nano_living_graph.yaml").read_text())
    for node in graph["nodes"]:
        node["path"] = sys.executable
        node["args"] = shlex.join(
            [
                "-u",
                str(snapshot / "nano_living_node.py"),
                node["id"].removeprefix("living_"),
            ]
        )
        node["env"] = {
            "NANO_LIVING_CONFIG": str(out / "config.json"),
            "PYTHONPATH": os.pathsep.join(
                [str(snapshot), str(args.aisle.resolve() / "src")]
            ),
            "PYTHONUNBUFFERED": "1",
        }
    (out / "dataflow.yaml").write_text(yaml.safe_dump(graph, sort_keys=False))
    with (out / "dora.log").open("w") as log:
        process = subprocess.Popen(
            [cli, "run", str(out / "dataflow.yaml"), "--log-format", "json"],
            cwd=out,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        deadline = time.monotonic() + args.timeout if args.timeout else None

        def terminate(_signum, _frame):
            raise KeyboardInterrupt

        signal.signal(signal.SIGTERM, terminate)
        try:
            while process.poll() is None:
                failures = list(out.glob("failure-*.json"))
                if failures:
                    raise RuntimeError(failures[0].read_text())
                if deadline is not None and time.monotonic() > deadline:
                    raise TimeoutError("Dora rollout timed out")
                time.sleep(0.2)
        except BaseException:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            raise
    if process.returncode:
        raise RuntimeError(f"Dora failed; see {out}/dora.log")
    result = json.loads((out / "result.json").read_text())
    print(json.dumps(result))
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
