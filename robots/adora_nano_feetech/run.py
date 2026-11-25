"""Launch a supervised Dora graph for the Nano base, or a byte-level mock bus."""
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
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--mock", action="store_true")
    target.add_argument("--port", help="Explicit USB serial device; never auto-selected")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--owner-pid", type=int)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test and not args.mock:
        parser.error("Scripted motion is only available on the mock bus")
    if args.port and (not args.port.startswith("/dev/") or not Path(args.port).exists()):
        parser.error("Choose an existing /dev serial device")
    cli = shutil.which("dora")
    if (not cli or subprocess.check_output([cli, "--version"], text=True).strip() != "dora-cli 1.0.1"
            or importlib.metadata.version("dora-rs") != "1.0.1"):
        parser.error("Dora CLI and Python API 1.0.1 are required")
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if (root / "config.json").exists():
        parser.error("Use a new output directory")
    config = dict(vars(args), output=str(root), supervisor_pid=os.getpid())
    (root / "config.json").write_text(json.dumps(config, indent=2))
    folder = Path(__file__).resolve().parent
    graph = yaml.safe_load((folder / "dataflow.yaml").read_text())
    for node in graph["nodes"]:
        node["path"] = sys.executable
        node["args"] = shlex.join(["-u", str(folder / "node.py"), node["id"]])
        node["env"] = {"NANO_BASE_CONFIG": str(root / "config.json"), "PYTHONUNBUFFERED": "1"}
    graph_path = root / "dataflow.yaml"
    graph_path.write_text(yaml.safe_dump(graph, sort_keys=False))
    stopping = False
    def terminate(_signum, _frame):
        nonlocal stopping
        stopping = True
    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    error = None
    with (root / "dora.log").open("w") as log:
        child = subprocess.Popen([cli, "run", str(graph_path), "--log-format", "json"],
                                 cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        started = time.monotonic()
        try:
            while not stopping and child.poll() is None:
                if (root / "stop").exists():
                    break
                if args.owner_pid:
                    try:
                        os.kill(args.owner_pid, 0)
                        # The UI renews a lease. Closing/frozen Studio stops
                        # control even when the parent PID still exists.
                        age = time.time() - (root / "lease").stat().st_mtime
                    except (ProcessLookupError, FileNotFoundError):
                        break
                    if age > 1:
                        break
                failures = list(root.glob("failure-*.json"))
                if failures:
                    error = failures[0].read_text()
                    break
                if args.self_test:
                    if (root / "self-test-complete").exists():
                        break
                    if time.monotonic()-started > 40:
                        error = "Self-test timed out before the controller sequence completed"
                        break
                time.sleep(.05)
        except KeyboardInterrupt:
            pass
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
            elif child.returncode:
                error = f"Dora exited with {child.returncode}; see dora.log"
    if args.self_test and not error:
        history = root / "status.jsonl"
        samples = [json.loads(line) for line in history.read_text().splitlines()] if history.exists() else []
        final = json.loads((root / "status.json").read_text()) if (root / "status.json").exists() else {}
        forward = any(c["command_ticks"][0] < -50 and c["command_ticks"][2] > 50 for c in samples)
        strafe = any(c["command_ticks"][0] < -50 and c["command_ticks"][1] > 50 and c["command_ticks"][2] < -50 for c in samples)
        turn = any(min(c["command_ticks"]) > 50 for c in samples)
        released = samples and samples[-1]["command_ticks"] == [0, 0, 0]
        if not (forward and strafe and turn and released and final.get("stop_confirmed")):
            error = f"Self-test failed: forward={forward}, strafe={strafe}, turn={turn}, release={released}, stop={final.get('stop_confirmed')}"
        else:
            print(f"PASS: {len(samples)} Dora status samples; forward, strafe, turn, release and torque-off verified")
    if error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
