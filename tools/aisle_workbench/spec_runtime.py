"""Run a reviewed Octos app skill through the installed SPEC bridge and a Nexus vendor node."""

import argparse
from contextlib import closing
import fcntl
from functools import lru_cache
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import URLError
from urllib.request import urlopen
import uuid

from spec_catalog import digest, encode, resolved
from worker import ROOT, describe, simulate

HERE = Path(__file__).resolve().parent
VERB = "vendor.dorobot.aisle.task.run"


def tree_sha(files, root):
    return digest(
        encode(
            {
                p.relative_to(root)
                .as_posix(): hashlib.sha256(p.read_bytes())
                .hexdigest()
                for p in sorted(files)
                if p.is_file()
            }
        )
    )


def bridge_python():
    return os.environ.get("DOROBOT_SPEC_PYTHON", sys.executable)


@lru_cache(maxsize=1)
def runtime_identity():
    from aisle.sim import nexus_backend
    import nexus3d

    # The bridge may use a different virtual environment than the Nexus simulation.
    command = [
        bridge_python(),
        "-c",
        "import pathlib,octos_spec_bridge; print(pathlib.Path(octos_spec_bridge.__file__).parent)",
    ]
    check = subprocess.run(command, capture_output=True, text=True)
    if check.returncode:
        raise ValueError("Install octos-spec-bridge in DOROBOT_SPEC_PYTHON")
    bridge = Path(check.stdout.strip())
    src = Path(nexus_backend.__file__).resolve()
    engine = Path(nexus3d.__file__).resolve().parent
    adapter_files = [
        p
        for d in (HERE, ROOT / "tools/aisle-validation")
        for p in d.glob("*.py")
        if not p.name.startswith("test_")
    ]
    return dict(
        node_sha256=digest(Path(__file__).read_bytes()),
        studio_adapter_sha256=tree_sha(adapter_files, ROOT),
        aisle_backend_sha256=tree_sha(
            [src, src.with_name("nexus_physics.toml")], src.parent
        ),
        nexus_sha256=tree_sha(
            [*engine.glob("*.so"), *engine.glob("*.pyd"), *engine.glob("*.py")], engine
        ),
        bridge_sha256=tree_sha(bridge.glob("*.py"), bridge),
    )


def lock_selection(pin, robot):
    return {"package": resolved(pin, robot), "runtime": runtime_identity()}


def verify_trial(trial, result, plan):
    """Re-resolve the local installation against the controller and both recorded endpoints."""
    pin = plan["spec_skill"]
    if (
        lock_selection(pin["package"], plan["robot"]) != pin
        or result.get("spec_skill") != pin
    ):
        raise ValueError("The installed SPEC runtime differs from the simulation")
    raw = (trial / "spec-receipt.json").read_bytes()
    receipt = json.loads(raw)
    measurement = (trial / "spec-measurement.json").read_bytes()
    facts = {
        key: value
        for key, value in result.items()
        if key not in ("spec_skill", "spec_request_id", "spec_receipt_sha256")
    }
    if not (
        digest(raw) == result.get("spec_receipt_sha256")
        and receipt["request_id"] == result.get("spec_request_id")
        and receipt["controller_sha256"]
        == digest((trial / "controller.json").read_bytes())
        and receipt["package"] == pin["package"]
        and receipt["package_sha256"] == pin["package"]["package_sha256"]
        and receipt["runtime"] == pin["runtime"]
        and receipt["verb"] == pin["package"]["verb"]
        and receipt["measured_result_sha256"] == digest(measurement)
        and json.loads(measurement) == facts
        and receipt["task_success"] is facts["task_success"]
    ):
        raise ValueError(
            "SPEC bridge receipt, package identity or independent Nexus measurement differs from the selected trial"
        )
    return {
        "package": pin["package"],
        "runtime": pin["runtime"],
        "request_id": receipt["request_id"],
        "receipt_sha256": digest(raw),
    }


def observation_inputs(plan):
    kind = plan.get("kind", "joint-inspection")
    if kind in ("visual-approach", "table-transfer"):
        return ["joint_encoders", "onboard_rgb"]
    if kind == "standing-inspection":
        return ["joint_encoders", "imu"]
    return ["joint_encoders"]


def config_args(args):
    return {
        k: {"local_path": str(v.resolve())} if isinstance(v, Path) else v
        for k, v in vars(args).items()
    }


def vendor(config):
    import pyarrow as pa
    from dora import Node

    c = json.loads(config.read_bytes())
    args = argparse.Namespace(
        **{
            k: Path(v["local_path"]) if isinstance(v, dict) and "local_path" in v else v
            for k, v in c["args"].items()
        }
    )
    expected = c["request"]
    plan = json.loads(args.controller.read_bytes())
    lock = plan["spec_skill"]
    if (
        expected["package_sha256"] != lock["package"]["package_sha256"]
        or expected["observation_inputs"] != observation_inputs(plan)
        or lock != lock_selection(lock["package"], args.robot)
        or digest(args.controller.read_bytes()) != expected["controller_sha256"]
    ):
        raise ValueError(
            "The simulation node resolved a different package, runtime or controller"
        )
    node = Node()
    done = False
    advert = {
        "spec_version": "1.0.0",
        "robot_id": args.robot,
        "commands": [{"verb": VERB, "safety_tier": "motion"}],
        "heartbeat_timeout_ms": 0,
    }

    def emit(port, value):
        node.send_output(port, pa.array([json.dumps(value)]))

    for event in node:
        if event["type"] in ("STOP", "ERROR"):
            break
        if event["type"] != "INPUT":
            continue
        if event["id"] == "advert_tick":
            emit("capabilities", advert)
            continue
        request = json.loads(event["value"][0].as_py())
        reply = dict(
            request_id=request["request_id"], ok=False, code="REJECTED", msg="", data={}
        )
        try:
            sensors = request.get("params", {}).get("observation_inputs")
            if not isinstance(sensors, list) or any(
                v not in lock["package"]["sensors"] for v in sensors
            ):
                reply["code"] = "OBSERVATION_DENIED"
                reply["msg"] = (
                    "Requested observation is outside the installed skill contract"
                )
                with (args.out / "spec-denials.jsonl").open("a") as stream:
                    stream.write(
                        json.dumps(
                            dict(
                                request_id=request["request_id"],
                                code=reply["code"],
                                observation_inputs=sensors,
                            )
                        )
                        + "\n"
                    )
                emit("cmd_response", reply)
                continue
            if (
                done
                or request["target"] != args.robot
                or request["verb"] != VERB
                or request["params"] != expected
            ):
                raise ValueError(
                    "Request differs from the prepared trial or was already executed"
                )
            done = True
            config.with_name("received-request.json").write_bytes(
                encode(
                    dict(
                        request_id=request["request_id"],
                        target=request["target"],
                        verb=request["verb"],
                        params=request["params"],
                    )
                )
            )
            result = simulate(args, describe(args.robot))
            content = encode(result)
            config.with_name("measured-result.json").write_bytes(content)
            reply.update(
                ok=True,
                code="0",
                data={
                    **expected,
                    "runtime": lock["runtime"],
                    "result_sha256": digest(content),
                    "task_success": result["task_success"],
                },
            )
        except Exception as exc:
            reply["msg"] = str(exc)
        emit("cmd_response", reply)


def run_skill(args, plan):
    import yaml

    lock = plan["spec_skill"]
    cli = (
        os.getenv("DORA_BIN")
        or shutil.which("dora", path=str(Path(bridge_python()).parent))
        or shutil.which("dora")
    )
    if not cli:
        raise ValueError("Install the matched Dora CLI for the SPEC bridge")
    if lock["runtime"] != runtime_identity():
        raise ValueError(
            "The deployed node, bridge or simulation adapter differs from the prepared runtime"
        )
    with tempfile.TemporaryDirectory(prefix="aisle-spec-") as work:
        root = Path(work)
        snapshot = root / "package"
        pin = resolved(lock["package"], args.robot, snapshot)
        request = dict(
            package_sha256=pin["package_sha256"],
            controller_sha256=digest(args.controller.read_bytes()),
            run_id=uuid.uuid4().hex,
            observation_inputs=observation_inputs(plan),
        )
        config = root / "job.json"
        config.write_bytes(encode({"request": request, "args": config_args(args)}))
        with closing(socket.socket()) as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        # Use short relative launchers. Dora passes quoted filesystem paths differently across platforms.
        (root / "launch.py").write_text(
            "import os,runpy,sys\nsys.path.insert(0,os.environ['AISLE_SPEC_MODULES'])\nrunpy.run_module('spec_node',run_name='__main__')\n"
        )
        node_env = {"AISLE_SPEC_MODULES": str(HERE), "AISLE_SPEC_JOB": str(config)}
        graph = {
            "nodes": [
                {
                    "id": "vendor",
                    "path": sys.executable,
                    "args": "launch.py vendor",
                    "env": node_env,
                    "inputs": {
                        "cmd_request": "bridge/cmd_request",
                        "advert_tick": "dora/timer/millis/250",
                    },
                    "outputs": [
                        "cmd_response",
                        "capabilities",
                        "state",
                        "safety_event",
                    ],
                },
                {
                    "id": "bridge",
                    "path": bridge_python(),
                    "args": "launch.py bridge",
                    "env": {
                        **node_env,
                        "ROBOT_ID": args.robot,
                        "HTTP_PORT": str(port),
                        "HTTP_HOST": "127.0.0.1",
                        "CMD_TIMEOUT_S": "590",
                    },
                    "inputs": {
                        k: "vendor/" + k
                        for k in (
                            "cmd_response",
                            "capabilities",
                            "state",
                            "safety_event",
                        )
                    },
                    "outputs": ["cmd_request"],
                },
            ]
        }
        graph_path = root / "graph.yaml"
        graph_path.write_text(yaml.safe_dump(graph))
        endpoint = f"http://127.0.0.1:{port}"
        with (root / "owner.lock").open("wb") as owner, (
            args.out / "spec-graph.log"
        ).open("wb") as log:
            fcntl.flock(owner, fcntl.LOCK_EX)
            proc = subprocess.Popen(
                [cli, "run", str(graph_path), "--stop-after", "600s"],
                cwd=root,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                deadline = time.monotonic() + 40
                while proc.poll() is None and time.monotonic() < deadline:
                    try:
                        with urlopen(endpoint + "/tools", timeout=0.4) as response:
                            tools = json.load(response)["tools"]
                        if VERB in [t["name"] for t in tools]:
                            break
                    except (URLError, TimeoutError, ConnectionError):
                        pass
                    time.sleep(0.1)
                else:
                    raise ValueError(
                        "The simulation skill did not advertise its SPEC action. See spec-graph.log"
                    )
                output = subprocess.run(
                    [str(snapshot / "main"), "run_task"],
                    cwd=snapshot,
                    input=encode(request),
                    capture_output=True,
                    timeout=600,
                    check=True,
                    env=os.environ
                    | {
                        "AISLE_SPEC_BRIDGE_URL": endpoint,
                        "PYTHONDONTWRITEBYTECODE": "1",
                    },
                )
                wrapper = json.loads(output.stdout)
                if not wrapper["success"]:
                    raise ValueError(f"SPEC action failed: {wrapper['output']}")
                reply = json.loads(wrapper["output"])
                raw = (root / "measured-result.json").read_bytes()
                result = json.loads(raw)
                expected = {
                    **request,
                    "runtime": lock["runtime"],
                    "result_sha256": digest(raw),
                    "task_success": result["task_success"],
                }
                received = json.loads((root / "received-request.json").read_bytes())
                if received != dict(
                    request_id=reply.get("request_id"),
                    target=args.robot,
                    verb=pin["verb"],
                    params=request,
                ):
                    raise ValueError(
                        "The app skill reply does not match the node’s received command"
                    )
                if (
                    reply.get("ok") is not True
                    or not reply.get("request_id")
                    or reply["data"] != expected
                ):
                    raise ValueError(
                        "The deployment bridge returned a different trial identity or measurement"
                    )
                receipt = dict(
                    package=pin,
                    runtime=lock["runtime"],
                    verb=VERB,
                    request_id=reply["request_id"],
                    **request,
                    measured_result_sha256=digest(raw),
                    task_success=result["task_success"],
                    backend="Nexus",
                )
                (args.out / "spec-receipt.json").write_bytes(encode(receipt))
                (args.out / "spec-measurement.json").write_bytes(raw)
                return {
                    **result,
                    "spec_skill": lock,
                    "spec_request_id": reply["request_id"],
                    "spec_receipt_sha256": digest(encode(receipt)),
                }
            finally:
                (root / "owner-finishing").touch()
                if proc.poll() is None:
                    proc.send_signal(signal.SIGINT)
                    try:
                        proc.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait()


if __name__ == "__main__":
    vendor(Path(os.environ["AISLE_SPEC_JOB"]))
