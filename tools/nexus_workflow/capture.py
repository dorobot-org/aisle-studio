"""Collect synchronized Nexus takes from the calibrated SO-101 leader."""

import json
import math
import os
from pathlib import Path
from queue import Empty, Queue
import shutil
import socket
import subprocess
import sys
import threading
import time

from workflow_io import (
    FPS, JOINTS, SETTLE_FRAMES, TASK, FrameStream, JobStopped, PreviewWriter,
    integer, local_path, runtime_environment, score, stop_child, vector, write_json,
)

MAX_FRAMES = FPS * 60
CAPTURE_CONTROLS = {
    "reset": {"reset_required", "ready", "paused", "saved"},
    "pause": {"ready"},
    "resume": {"paused"},
    "step": {"paused"},
    "record": {"ready"},
    "finish": {"recording"},
    "save": {"review"},
    "discard": {"review"},
}


class Capture:
    def __init__(self, protocol, output, source_mode):
        self.protocol = protocol
        self.output = output
        self.pending = None
        self.latest = None
        self.settled_frames = 0
        self.frames = []
        self.take = None
        self.raw = None
        self.export = None
        self.initial_height = 0.0
        self.result = None
        self.source_mode = source_mode
        self.protocol.update(source_mode=source_mode)

    @property
    def pending_id(self):
        return self.pending[0] if self.pending is not None else None

    @property
    def phase(self):
        return self.protocol.state["phase"]

    def command(self, command_id, action):
        publish = self.protocol.update
        if action == "stop":
            self.protocol.request_stop(command_id)
        elif self.phase not in CAPTURE_CONTROLS.get(action, ()):
            publish(ack=command_id, message=f"Action {action!r} is unavailable while {self.phase}")
        elif action == "reset":
            self.pending = (command_id, action, self.latest["reset_id"] + 1, time.monotonic() + 30)
            self.settled_frames = 0
            (self.output / "reset-request").touch()
            (self.output / "pause").unlink(missing_ok=True)
            publish(phase="resetting", message="Waiting for Nexus reset and fresh leader samples",
                    frames=0, task_success=None)
        elif action in ("pause", "resume", "step"):
            self.pending = (command_id, action, self.latest["sim_tick"], time.monotonic() + 10)
            marker = self.output / ("step-request" if action == "step" else "pause")
            if action == "resume":
                marker.unlink(missing_ok=True)
            else:
                marker.touch()
            publish(message=f"Waiting for Nexus to confirm {action}")
        elif action == "record":
            if self.latest["leader_deg_percent"] is None:
                publish(ack=command_id, phase="reset_required", message="Leader input was lost. Reset after it returns.")
                return
            self.start_take()
            publish(ack=command_id, phase="recording", message="Recording synchronized Nexus samples",
                    frames=0, task_success=None)
        elif action == "finish":
            self.finish(ack=command_id)
        elif action == "discard":
            self.remove_take()
            publish(ack=command_id, phase="reset_required", message="Take discarded. Reset before recording again.",
                    frames=0, task_success=None)
        elif action == "save":
            self.begin_export(command_id)

    def start_take(self):
        take = self.output / f".episode-{self.latest['reset_id']:03}.pending"
        take.mkdir()
        self.take = take
        self.raw = (take / "frames.rgb").open("wb")
        self.frames = []

    def finish(self, **status):
        self.raw.close()
        self.raw = None
        if len(self.frames) < SETTLE_FRAMES:
            self.reject_take(f"Record at least {SETTLE_FRAMES} frames. Reset to try again.", **status)
            return
        frames = self.frames
        wall_seconds = (frames[-1]["capture_monotonic_ns"] - frames[0]["capture_monotonic_ns"]) / 1e9
        self.result = {
            **score(frames, self.initial_height), "frames": len(frames), "source_mode": self.source_mode,
            "wall_fps": round((len(frames) - 1) / max(wall_seconds, 1e-6), 2),
        }
        write_json(self.take / "trace.json", {
            "joint_names": JOINTS, "sample_hz_simulated": FPS, "task": TASK, "source_mode": self.source_mode,
            "gripper_limits_rad": frames[0]["gripper_limits_rad"], "frames": frames,
        })
        write_json(self.take / "result.json", self.result)
        self.protocol.update(phase="review", message="Review the take, then save it as LeRobot v3 or discard it",
                             frames=len(frames), task_success=self.result["task_success"], **status)

    def reject_take(self, message, **status):
        self.remove_take()
        self.protocol.update(phase="reset_required", message=message, frames=0, task_success=None, **status)

    def remove_take(self):
        if self.raw is not None:
            self.raw.close()
            self.raw = None
        if self.take is not None:
            shutil.rmtree(self.take)
            self.take = None
        self.frames = []
        self.result = None

    def begin_export(self, command_id):
        helper = Path(__file__).with_name("artifacts.py")
        with (self.take / "export.log").open("w") as log:
            self.export = subprocess.Popen(
                [sys.executable, "-u", str(helper), "export", str(self.take)],
                stdout=log, stderr=subprocess.STDOUT,
            )
        self.protocol.update(ack=command_id, phase="saving", message="Writing and verifying LeRobot v3 dataset")

    def check_export(self):
        if self.export is None or self.export.poll() is None:
            return
        code = self.export.returncode
        self.export = None
        if code != 0:
            log = self.take / "export.log"
            print(log.read_text(errors="replace"), end="", flush=True)
            dataset = self.take / "dataset"
            if dataset.exists():
                shutil.rmtree(dataset)
            self.protocol.update(phase="review", message=f"LeRobot export failed; retry or discard. See {log}")
            return
        self.protocol.check_stop()
        destination = self.take.with_name(self.take.name.removeprefix(".").removesuffix(".pending"))
        dataset = destination / "dataset"
        write_json(self.take / "result.json", {**self.result, "dataset": str(dataset)})
        self.take.rename(destination)
        self.take = None
        self.protocol.update(phase="saved", message="Verified LeRobot v3 take saved. Reset for another take.",
                             dataset=str(dataset))
        self.frames = []

    def advance_pending(self, packet):
        if self.pending is None:
            return
        command_id, action, previous, deadline = self.pending
        ready = packet["leader_deg_percent"] is not None
        paused = packet["paused"]
        if time.monotonic() > deadline:
            raise RuntimeError(f"Nexus did not confirm {action}")
        if action == "reset":
            self.settled_frames = self.settled_frames + 1 if packet["reset_id"] == previous and ready and not paused else 0
            if self.settled_frames < SETTLE_FRAMES:
                return
            self.initial_height = packet["cube_xyz"][2]
            phase, message = "ready", "Nexus reset confirmed and settled. Ready to record."
        elif action == "pause" and paused:
            phase, message = "paused", "Nexus paused"
        elif action == "resume" and not paused:
            phase = "ready" if ready else "reset_required"
            message = "Nexus resumed" if ready else "Leader input was lost. Reset after it returns."
        elif action == "step" and paused and packet["sim_tick"] == previous + 1:
            phase, message = "paused", "Nexus advanced one physics step"
        else:
            return
        self.pending = None
        self.protocol.update(ack=max(command_id, self.protocol.state["ack"]), phase=phase, message=message)

    def frame(self, packet, rgb):
        ready = packet.get("leader_deg_percent") is not None
        if ready:
            vector(packet["leader_deg_percent"], 6, "leader input")
        for name, count in (
            ("state_rad", 6), ("target_rad", 6), ("gripper_limits_rad", 2),
            ("cube_xyz", 3), ("plate_xyz", 3),
        ):
            vector(packet.get(name), count, name)
        limits = packet["gripper_limits_rad"]
        if limits[0] >= limits[1] or type(packet.get("paused")) is not bool:
            raise ValueError("Nexus capture provided invalid gripper limits or pause state")
        for name in ("seq", "reset_id", "sim_tick", "capture_monotonic_ns"):
            integer(packet.get(name), name)

        previous = self.latest
        self.latest = packet
        if self.phase == "starting" and ready:
            self.protocol.update(phase="reset_required", message="Leader input confirmed. Reset the scene to record.")
        self.advance_pending(packet)
        if self.phase == "ready" and not ready and self.pending is None:
            self.protocol.update(phase="reset_required", message="Leader input was lost. Reset after it returns.")
        if self.phase == "recording":
            paired = (
                ready and not packet["paused"] and packet["reset_id"] == previous["reset_id"]
                and packet["seq"] == previous["seq"] + 1 and packet["sim_tick"] == previous["sim_tick"] + 5
                and packet["capture_monotonic_ns"] > previous["capture_monotonic_ns"]
            )
            if not paired:
                self.reject_take("A synchronized frame or leader input was lost. Reset to record again.")
            else:
                self.raw.write(rgb)
                self.frames.append({
                    "seq": packet["seq"], "timestamp": len(self.frames) / FPS,
                    "capture_monotonic_ns": packet["capture_monotonic_ns"],
                    "observation_state_rad": packet["state_rad"], "accepted_target_rad": packet["target_rad"],
                    "gripper_limits_rad": limits, "cube_xyz": packet["cube_xyz"], "plate_xyz": packet["plate_xyz"],
                })
                self.protocol.update(frames=len(self.frames))
                if len(self.frames) >= MAX_FRAMES:
                    self.finish()
        if self.phase not in ("review", "saving"):
            self.protocol.update(positions=packet["state_rad"], targets=packet["target_rad"])

    def close(self):
        if self.export is not None:
            stop_child(self.export)
            self.export = None
        self.remove_take()


def leader_calibration(value):
    selected = local_path(value, "calibration_dir")
    candidates = (selected / "leader.json", selected / "so101_leader.json") if selected.is_dir() else (selected,)
    for calibration in candidates:
        if calibration.is_file() and calibration.suffix == ".json":
            return calibration.parent, calibration.stem
    raise ValueError("Select a saved SO-101 calibration JSON or a folder containing leader.json or so101_leader.json")


def leader_input(config, directory, stop, errors):
    leader = None
    fd = None
    try:
        if not config.get("synthetic", False):
            from lerobot.teleoperators.so_leader import SO101Leader, SO101LeaderConfig

            leader = SO101Leader(SO101LeaderConfig(
                port=str(config["leader_port"]), id=config["leader_id"], calibration_dir=config["calibration_dir"],
                use_degrees=True,
            ))
            if set(leader.calibration) != set(JOINTS):
                raise ValueError("Saved leader calibration must contain all six SO-101 axes")
            if stop.is_set():
                return
            leader.connect(calibrate=False)
            if not leader.is_calibrated:
                raise ValueError("The connected SO-101 leader does not match the saved calibration")
        # Linux's AF_UNIX path has a fixed limit. Open the job directory to address
        # the runner's relative socket even when the selected output path is long.
        fd = os.open(directory, os.O_DIRECTORY)
        path = f"/proc/self/fd/{fd}/leader.sock"
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as channel:
            channel.setblocking(False)
            channel.connect(path)
            deadline = time.monotonic()
            sample = failures = 0
            while not stop.is_set():
                try:
                    if leader is None:
                        t = sample / 50
                        values = [8 * math.sin(t), -8 + 5 * math.sin(t * .9), 6 * math.sin(t * .6),
                                  -4 * math.sin(t * .45), 5 * math.sin(t * .5), 45 + 10 * math.sin(t * .75)]
                    else:
                        positions = leader.get_action()
                        values = [float(positions[f"{name}.pos"]) for name in JOINTS]
                    vector(values, 6, "SO-101 leader sample")
                    channel.send(json.dumps(values, allow_nan=False).encode())
                    failures = 0
                except BlockingIOError:
                    pass
                except Exception:
                    failures += 1
                    if failures >= 5:
                        raise
                sample += 1
                deadline = max(deadline + 1 / 50, time.monotonic())
                stop.wait(deadline - time.monotonic())
    except Exception as error:
        if not stop.is_set():
            errors.put(error)
    finally:
        if fd is not None:
            os.close(fd)
        if leader is not None and leader.is_connected:
            leader.disconnect()


def run(config, output, protocol):
    synthetic = config.get("synthetic", False)
    if type(synthetic) is not bool:
        raise ValueError("synthetic must be a boolean")
    if not synthetic:
        config["leader_port"] = local_path(config.get("leader_port"), "leader_port")
        if not config["leader_port"].exists():
            raise ValueError(f"SO-101 leader port is missing: {config['leader_port']}")
        config["calibration_dir"], config["leader_id"] = leader_calibration(config.get("calibration_dir"))

    session = Capture(protocol, output, source_mode="synthetic" if synthetic else "calibrated_leader")
    stream = reader = None
    leader_errors = Queue()
    log_path = output / "nexus.log"
    with log_path.open("w") as log, PreviewWriter(protocol) as preview:
        native = subprocess.Popen(
            [config["runtime"], "capture", config["model"], config["scene"], "."],
            cwd=output, stdout=subprocess.PIPE, stderr=log, env=runtime_environment(),
        )
        try:
            stream = FrameStream(native.stdout)
            deadline = time.monotonic() + 40
            last_frame = time.monotonic()
            while True:
                if (command := protocol.command(session.pending_id)) is not None:
                    session.command(*command)
                if protocol.stop.is_set():
                    raise JobStopped
                if reader is None and (output / "leader.sock").exists():
                    reader = threading.Thread(
                        target=leader_input, args=(config, output, protocol.stop, leader_errors), name="so101-leader",
                    )
                    reader.start()
                try:
                    error = leader_errors.get_nowait()
                except Empty:
                    pass
                else:
                    raise RuntimeError(f"SO-101 leader input failed: {error}") from error
                if session.phase == "starting" and time.monotonic() > deadline:
                    raise RuntimeError(f"Timeout waiting for synchronized Nexus and leader input. See {log_path}")
                if time.monotonic() - last_frame > 40:
                    raise RuntimeError(f"Timeout waiting for a Nexus camera frame. See {log_path}")
                if session.pending is not None and time.monotonic() > session.pending[3]:
                    raise RuntimeError(f"Nexus did not confirm {session.pending[1]}. See {log_path}")
                try:
                    data = stream.next()
                except EOFError as error:
                    raise RuntimeError(f"Nexus stopped streaming (exit {native.poll()}). See {log_path}") from error
                if data is not None:
                    packet, rgb = data
                    session.frame(packet, rgb)
                    if session.phase not in ("review", "saving"):
                        preview.offer((packet["seq"], rgb))
                    last_frame = time.monotonic()
                session.check_export()
        finally:
            protocol.stop.set()
            (output / "stop").touch()
            if stream is not None:
                stream.stop.set()
            stop_child(native, grace=1.0)
            if stream is not None:
                stream.close()
            socket_path = output / "leader.sock"
            if socket_path.is_socket():
                socket_path.unlink(missing_ok=True)
            if reader is not None:
                reader.join()
            session.close()
