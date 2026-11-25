"""Train and run a local LeRobot ACT policy against the Nexus SO-101 scene."""

import json
import math
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

from artifacts import ACTION, CAMERA, REPO_ID, STATE
from workflow_io import (
    FPS, HEIGHT, RGB_BYTES, WIDTH, FrameStream, PreviewWriter, child_environment, in_lerobot_units,
    in_runtime_units, integer, local_path, read_json, runtime_environment, score, stop_child, vector, write_json,
)

PROGRESS = re.compile(r"\bTraining:.*?\b(\d+)\s*/\s*(\d+)\s*\[")
STEP = re.compile(r"\bstep:(\d+)\b")
LOSS = re.compile(r"(?<!\w)loss:([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)(?:\s|$)")


def training_progress(line, total):
    exact = PROGRESS.search(line)
    count = STEP.search(line)
    loss = LOSS.search(line)
    step = None
    if exact and int(exact[2]) == total:
        step = int(exact[1])
    elif count:
        step = int(count[1])
    value = float(loss[1]) if loss else None
    return step if step is None or 0 <= step <= total else None, value if value is not None and math.isfinite(value) else None


def check_policy_features(config):
    if not isinstance(config, dict) or config.get("type") != "act" or config.get("n_obs_steps") != 1:
        raise ValueError("Choose a local LeRobot ACT checkpoint with one observation step")
    for section, features in (
        ("input_features", {STATE: ("STATE", [6]), CAMERA: ("VISUAL", [3, HEIGHT, WIDTH])}),
        ("output_features", {ACTION: ("ACTION", [6])}),
    ):
        selected = config.get(section)
        if not isinstance(selected, dict):
            raise ValueError(f"ACT checkpoint is missing {section}")
        for key, (kind, shape) in features.items():
            actual = selected.get(key, {})
            if not isinstance(actual, dict) or actual.get("type") != kind or actual.get("shape") != shape:
                raise ValueError(f"ACT checkpoint must define {key} with shape {shape}")
        if selected.keys() != features.keys():
            raise ValueError(f"ACT checkpoint has unexpected {section}: {', '.join(selected.keys() - features.keys())}")


def checkpoint_path(selected):
    for candidate in (selected, selected / "pretrained_model", selected / "checkpoints/last/pretrained_model"):
        if not (candidate / "config.json").is_file():
            continue
        path = candidate.resolve()
        check_policy_features(read_json(path / "config.json"))
        model_file = path / "model.safetensors"
        if not model_file.is_file() or model_file.stat().st_size == 0:
            raise ValueError(f"ACT checkpoint is missing model.safetensors: {path}")
        for name in ("policy_preprocessor", "policy_postprocessor"):
            processor = path / f"{name}.json"
            if not processor.is_file():
                raise ValueError(f"ACT checkpoint is missing {processor.name}: {path}")
            pipeline = read_json(processor)
            steps = pipeline.get("steps") if isinstance(pipeline, dict) else None
            if not isinstance(steps, list) or not steps or any(not isinstance(step, dict) for step in steps):
                raise ValueError(f"Invalid ACT processor pipeline in {processor}")
            for step in steps:
                state_file = step.get("state_file")
                if state_file is not None and (
                    not isinstance(state_file, str) or Path(state_file).name != state_file
                    or not (path / state_file).is_file()
                ):
                    raise ValueError(f"Missing saved processor weights in {processor}: {state_file}")
        return path
    raise ValueError(f"Choose an ACT pretrained_model directory, checkpoint directory, or training output: {selected}")


def make_actor(checkpoint):
    import numpy as np
    import torch
    from lerobot.configs.policies import PreTrainedConfig
    from lerobot.policies import make_pre_post_processors
    from lerobot.policies.act.modeling_act import ACTPolicy

    if not torch.cuda.is_available():
        torch.set_num_threads(min(4, torch.get_num_threads()))
    config = PreTrainedConfig.from_pretrained(checkpoint, local_files_only=True)
    # The checkpoint has all backbone tensors. Skip torchvision network fetches.
    config.pretrained_backbone_weights = None
    config.n_action_steps = 1
    actor = ACTPolicy.from_pretrained(checkpoint, config=config, local_files_only=True, strict=True)
    actor.reset()
    preprocess, postprocess = make_pre_post_processors(
        config, pretrained_path=checkpoint,
        preprocessor_overrides={"device_processor": {"device": config.device}},
    )

    def predict(frame, rgb):
        limits = frame["joint_limits_rad"]
        position = np.asarray(in_lerobot_units(frame["state_rad"], limits[-1]), dtype=np.float32)
        image = np.frombuffer(rgb, dtype=np.uint8).copy().reshape(HEIGHT, WIDTH, 3)
        with torch.inference_mode():
            batch = preprocess({
                STATE: torch.from_numpy(position),
                CAMERA: torch.from_numpy(image).permute(2, 0, 1).float().div_(255),
            })
            action = postprocess(actor.select_action(batch)).detach().cpu()
        if tuple(action.shape) not in ((6,), (1, 6)):
            raise ValueError(f"ACT produced an invalid action shape: {tuple(action.shape)}")
        values = vector(action.reshape(6).tolist(), 6, "ACT action")
        target = in_runtime_units(values, limits[-1])
        return [min(hi, max(lo, value)) for value, (lo, hi) in zip(target, limits, strict=True)]

    return predict


def run_logged(args, log_path, protocol, total=None):
    with log_path.open("w") as log:
        child = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT, env=child_environment())
    partial = ""
    try:
        with log_path.open(errors="replace", newline="") as reader:
            while True:
                protocol.check_stop()
                code = child.poll()
                lines = re.split("[\r\n]", partial + reader.read())
                partial = lines.pop()
                if code is not None and partial:
                    lines.append(partial)
                if total is not None:
                    steps, loss = protocol.state["steps"], protocol.state.get("loss")
                    last = steps, loss
                    for line in lines:
                        step, value = training_progress(line, total)
                        if step is not None:
                            steps = max(steps, step)
                        if value is not None:
                            loss = value
                    if (steps, loss) != last:
                        protocol.update(steps=steps, loss=loss, message="Training ACT from synchronized Nexus episodes")
                if code is not None:
                    if code != 0:
                        tail = reader.name
                        with log_path.open("rb") as details:
                            details.seek(max(0, log_path.stat().st_size - 4096))
                            errors = details.read().decode(errors="replace").splitlines()
                        if errors:
                            tail = errors[-1]
                        raise RuntimeError(f"LeRobot subprocess exited with status {code}: {tail}. See {log_path}")
                    return
                protocol.stop.wait(0.1)
    finally:
        stop_child(child)


def train(config, output, protocol):
    selected = local_path(config.get("dataset"), "dataset").resolve()
    steps = integer(config.get("steps"), "ACT training step count", minimum=1)
    protocol.update(phase="training", total_steps=steps, message="Validating selected LeRobot v3 episodes")
    stage = output / ".dataset-preparation.pending"
    stage.mkdir()
    try:
        args = [sys.executable, "-u", str(Path(__file__).with_name("artifacts.py")), "prepare", selected, stage]
        run_logged(args, output / "dataset-preparation.log", protocol)
        protocol.check_stop()
        provenance = read_json(stage / "dataset-sources.json")
        source = Path(provenance["artifact"])
        artifact = source if source.is_absolute() else output / "training-dataset"
        provenance["artifact"] = str(artifact)
        write_json(output / "dataset-sources.json", provenance)
        if not source.is_absolute():
            (stage / "dataset").rename(artifact)
    finally:
        shutil.rmtree(stage)

    protocol.check_stop()
    pending_training = output / ".training.pending"
    protocol.update(message=f"Starting ACT with {provenance['total_episodes']} selected episodes")
    interval = max(1, min(50, steps // 20))
    train_args = [
        sys.executable, "-u", "-m", "lerobot.scripts.lerobot_train",
        f"--dataset.repo_id={REPO_ID}", f"--dataset.root={artifact}", "--dataset.video_backend=pyav",
        "--policy.type=act", "--policy.push_to_hub=false", "--policy.pretrained_backbone_weights=null",
        "--policy.chunk_size=20", "--policy.n_action_steps=1",
        f"--output_dir={pending_training}", f"--steps={steps}", f"--save_freq={steps}",
        f"--log_freq={interval}", f"--batch_size={min(8, provenance['total_frames'])}", "--num_workers=0",
    ]
    run_logged(train_args, output / "training.log", protocol, total=steps)
    protocol.check_stop()
    protocol.update(message="Validating the saved ACT checkpoint")
    checkpoint = checkpoint_path(pending_training)
    predict = make_actor(checkpoint)
    gripper = [0.0, 1.0]
    predict({
        "state_rad": in_runtime_units(provenance["initial_state_deg_percent"], gripper),
        "joint_limits_rad": [[-math.pi, math.pi]] * 5 + [gripper],
    }, bytes(RGB_BYTES))
    protocol.check_stop()
    write_json(checkpoint / "nexus_provenance.json", provenance)
    training = output / "training"
    relative_checkpoint = checkpoint.relative_to(pending_training.resolve())
    pending_training.rename(training)
    protocol.update(phase="completed", message="ACT training complete. Validated checkpoint is ready to run.",
                    steps=steps, checkpoint=str(training / relative_checkpoint))


def rollout_initial(config, checkpoint):
    provenance = checkpoint / "nexus_provenance.json"
    if not provenance.is_file():
        return [0.0] * 5 + [0.6]
    first = read_json(provenance)["initial_state_deg_percent"]
    limits = ET.parse(config["model"]).find("./joint[@name='gripper']/limit")
    if limits is None:
        raise ValueError("SO-101 model is missing gripper limits")
    lo, hi = float(limits.attrib["lower"]), float(limits.attrib["upper"])
    if not math.isfinite(lo) or not math.isfinite(hi) or lo >= hi:
        raise ValueError("SO-101 model has invalid gripper limits")
    return in_runtime_units(vector(first, 6, STATE), (lo, hi))


def rollout_frame(packet, sequence):
    if packet.get("seq") != sequence:
        raise ValueError(f"Unexpected Nexus observation sequence {packet.get('seq')}; expected {sequence}")
    for name, count in (("state_rad", 6), ("cube_xyz", 3), ("plate_xyz", 3)):
        vector(packet.get(name), count, name)
    limits = packet.get("joint_limits_rad")
    if not isinstance(limits, list) or len(limits) != 6:
        raise ValueError("Nexus rollout is missing SO-101 joint limits")
    for limit in limits:
        lo, hi = vector(limit, 2, "joint limits")
        if lo >= hi:
            raise ValueError("Nexus reported invalid joint limits")


def receive_observation(stream, protocol, log_path):
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        protocol.check_stop()
        try:
            if (data := stream.next()) is not None:
                return data
        except EOFError as error:
            raise RuntimeError(f"Nexus ended before the requested rollout completed. See {log_path}") from error
    raise RuntimeError(f"Timed out waiting for a Nexus rollout observation. See {log_path}")


def rollout(config, output, protocol):
    steps = integer(config.get("steps"), "rollout step count", minimum=1)
    protocol.update(phase="running", total_steps=steps, message="Loading and validating the local ACT checkpoint")
    checkpoint = checkpoint_path(local_path(config.get("checkpoint"), "checkpoint"))
    predict = make_actor(checkpoint)
    initial = rollout_initial(config, checkpoint)
    initial_path = output / "initial-joints.json"
    write_json(initial_path, initial)
    protocol.check_stop()

    import av
    import numpy as np

    trace_path = output / "rollout-trace.jsonl"
    log_path = output / "nexus.log"
    pending_video = output / ".rollout.pending.mp4"
    destination = output / "rollout.mp4"
    frames = []
    prediction_ms = []
    last_target = initial
    started = time.monotonic()
    try:
        with log_path.open("w") as log, trace_path.open("w", buffering=1) as trace, av.open(
            str(pending_video), "w", options={"movflags": "+faststart"},
        ) as video, PreviewWriter(protocol) as preview:
            encoder = video.add_stream("libx264", rate=FPS)
            encoder.width, encoder.height = WIDTH, HEIGHT
            encoder.pix_fmt, encoder.thread_count = "yuv420p", 2
            encoder.options = {"preset": "veryfast", "crf": "22"}
            native = subprocess.Popen(
                [config["runtime"], "rollout", config["model"], config["scene"], initial_path, str(steps)],
                stdout=subprocess.PIPE, stdin=subprocess.PIPE, stderr=log, env=runtime_environment(),
            )
            stream = None
            try:
                stream = FrameStream(native.stdout)
                for sequence in range(steps + 1):
                    frame, rgb = receive_observation(stream, protocol, log_path)
                    rollout_frame(frame, sequence)
                    if sequence == 0 and any(
                        abs(measured - min(hi, max(lo, requested))) > 1e-4
                        for measured, requested, (lo, hi) in zip(
                            frame["state_rad"], initial, frame["joint_limits_rad"], strict=True,
                        )
                    ):
                        raise ValueError("Nexus did not start the rollout at the requested SO-101 pose")
                    if sequence < steps:
                        prediction_started = time.perf_counter()
                        last_target = predict(frame, rgb)
                        prediction_ms.append((time.perf_counter() - prediction_started) * 1_000)
                        protocol.check_stop()
                        command = {"seq": sequence, "target_rad": last_target}
                        native.stdin.write((json.dumps(command, allow_nan=False) + "\n").encode())
                        native.stdin.flush()
                        frame["accepted_target_rad"] = last_target
                    frames.append(frame)
                    trace.write(json.dumps(frame, allow_nan=False, separators=(",", ":")) + "\n")
                    image = np.frombuffer(rgb, dtype=np.uint8).reshape(HEIGHT, WIDTH, 3)
                    video.mux(encoder.encode(av.VideoFrame.from_ndarray(image, format="rgb24")))
                    preview.offer((sequence, rgb))
                    protocol.update(
                        message="Running closed-loop ACT in Nexus", frames=len(frames), steps=sequence,
                        positions=frame["state_rad"], targets=last_target,
                    )
                if native.wait(timeout=5) != 0:
                    raise RuntimeError(f"Nexus rollout exited with status {native.returncode}. See {log_path}")
                video.mux(encoder.encode())
            finally:
                native.stdin.close()
                stop_child(native)
                if stream is not None:
                    stream.close()
        protocol.check_stop()
        with av.open(str(pending_video)) as check:
            camera = check.streams.video[0]
            if camera.frames != len(frames) or (camera.width, camera.height) != (WIDTH, HEIGHT):
                raise ValueError("Saved rollout video did not preserve the synchronized Nexus frames")
        elapsed = time.monotonic() - started
        result = {
            **score(frames, frames[0]["cube_xyz"][2]), "controller": "ACT",
            "action_steps": steps, "video_frames": len(frames), "simulation_fps": FPS,
            "wall_seconds": round(elapsed, 3), "wall_fps": round(steps / max(elapsed, 1e-6), 2),
            "policy_action_p50_ms": round(statistics.median(prediction_ms), 2),
            "policy_action_p95_ms": round(sorted(prediction_ms)[math.ceil(0.95 * steps) - 1], 2),
            "checkpoint": str(checkpoint), "video": str(destination),
        }
        write_json(output / "result.json", result)
        pending_video.replace(destination)
        protocol.update(phase="completed", message="Closed-loop Nexus rollout complete. Video saved.",
                        task_success=result["task_success"], video=str(destination))
    finally:
        pending_video.unlink(missing_ok=True)
