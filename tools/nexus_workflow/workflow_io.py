"""Filesystem and process protocol shared by the Nexus workflow jobs."""

import json
import math
import os
from pathlib import Path
from queue import Empty, Full, Queue
import struct
import subprocess
import threading

FPS = 20
WIDTH, HEIGHT = 640, 480
RGB_BYTES = WIDTH * HEIGHT * 3
JOINTS = ("shoulder_pan", "shoulder_lift", "elbow_flex", "wrist_flex", "wrist_roll", "gripper")
TASK = "Pick up the red cube and place it on the blue plate"
SETTLE_FRAMES = 8


def atomic_bytes(path, data):
    temp = path.with_name(f".{path.name}.tmp")
    temp.write_bytes(data)
    temp.replace(path)


def write_json(path, value):
    atomic_bytes(path, (json.dumps(value, allow_nan=False, separators=(",", ":")) + "\n").encode())


def read_json(path):
    return json.loads(path.read_text())


def local_path(value, name):
    if not isinstance(value, str) or not Path(value).is_absolute():
        raise ValueError(f"{name} must be an absolute local path")
    return Path(value)


def vector(value, count, name):
    if not isinstance(value, list) or len(value) != count or any(
        type(component) not in (int, float) or not math.isfinite(component) for component in value
    ):
        raise ValueError(f"Expected {count} finite values for {name}")
    return value


def integer(value, name, minimum=0):
    if type(value) is not int or not minimum <= value <= (1 << 64) - 1:
        raise ValueError(f"Invalid {name}: expected an integer of at least {minimum}")
    return value


class JobStopped(Exception):
    """The operator or Studio requested cancellation."""


class Protocol:
    def __init__(self, directory):
        self.directory = directory
        self.state = {
            "version": 1, "phase": "starting", "message": "Starting local Nexus worker",
            "ack": 0, "frames": 0, "steps": 0, "total_steps": 0,
        }
        self.stop = threading.Event()
        self.update()

    def update(self, **changes):
        for key, value in changes.items():
            if value is None:
                self.state.pop(key, None)
            else:
                self.state[key] = value
        write_json(self.directory / "status.json", self.state)

    def write_preview(self, sequence, rgb):
        atomic_bytes(self.directory / "preview.rgb", struct.pack(">QHH", sequence, WIDTH, HEIGHT) + rgb)

    def command(self, pending_id=None):
        try:
            data = read_json(self.directory / "command.json")
        except (FileNotFoundError, ValueError):
            return None
        if not isinstance(data, dict):
            return None
        command_id, action = data.get("id"), data.get("action")
        if type(command_id) is not int or not self.state["ack"] < command_id <= (1 << 64) - 1:
            return None
        if pending_id is not None and (action != "stop" or command_id <= pending_id):
            return None
        if type(data.get("version")) is not int or data["version"] != 1:
            self.update(ack=command_id, message="Unsupported command version")
            return None
        if not isinstance(action, str):
            self.update(ack=command_id, message="Command action must be a string")
            return None
        return command_id, action

    def request_stop(self, command_id=None):
        if command_id is not None:
            self.update(ack=command_id, phase="stopping", message="Stopping local Nexus job")
        self.stop.set()

    def check_stop(self):
        command = self.command()
        if command is not None:
            command_id, action = command
            if action == "stop":
                self.request_stop(command_id)
            else:
                self.update(ack=command_id, message=f"Action {action!r} is unavailable during this job")
        if self.stop.is_set():
            raise JobStopped


class PreviewWriter:
    """Coalesce late previews so the UI never holds up physics or recording."""

    def __init__(self, protocol):
        self.protocol = protocol
        self.frames = Queue(maxsize=1)
        self.stop = threading.Event()
        self.error = None
        self.thread = threading.Thread(target=self.write, name="nexus-preview")

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, error_type, *_):
        self.stop.set()
        self.thread.join()
        if self.error is not None and error_type is None:
            raise self.error

    def offer(self, frame):
        if self.error is not None:
            raise self.error
        try:
            self.frames.get_nowait()
        except Empty:
            pass
        self.frames.put_nowait(frame)

    def write(self):
        try:
            while not self.stop.is_set() or not self.frames.empty():
                try:
                    frame = self.frames.get(timeout=0.05)
                except Empty:
                    continue
                self.protocol.write_preview(*frame)
        except Exception as error:
            self.error = error


def read_frame(stream):
    line = stream.readline(16_384)
    if not line:
        return None
    if not line.endswith(b"\n"):
        raise ValueError("Nexus frame header is too long or truncated")
    packet = json.loads(line)
    if not isinstance(packet, dict) or packet.get("rgb_bytes") != RGB_BYTES:
        raise ValueError(f"Nexus must provide {WIDTH}×{HEIGHT} packed RGB frames")
    integer(packet.get("seq"), "Nexus frame sequence")
    rgb = stream.read(RGB_BYTES)
    if len(rgb) != RGB_BYTES:
        raise ValueError("Nexus RGB frame is truncated")
    return packet, rgb


class FrameStream:
    """Read complete frames off the child pipe while the worker polls commands."""

    def __init__(self, stream):
        self.stream = stream
        self.frames = Queue(maxsize=2)
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.read, name="nexus-frames")
        self.thread.start()

    def offer(self, frame):
        while not self.stop.is_set():
            try:
                self.frames.put(frame, timeout=0.05)
                return
            except Full:
                pass

    def read(self):
        try:
            while not self.stop.is_set():
                frame = read_frame(self.stream)
                self.offer(frame)
                if frame is None:
                    return
        except Exception as error:
            self.offer(error)

    def next(self):
        try:
            frame = self.frames.get(timeout=0.05)
        except Empty:
            return None
        if isinstance(frame, Exception):
            raise frame
        if frame is None:
            raise EOFError("Nexus closed its observation stream")
        return frame

    def close(self):
        self.stop.set()
        self.thread.join()
        self.stream.close()


def stop_child(child, grace=0.0):
    try:
        return child.wait(timeout=grace)
    except subprocess.TimeoutExpired:
        child.terminate()
    try:
        return child.wait(timeout=1.0)
    except subprocess.TimeoutExpired:
        child.kill()
        return child.wait()


def child_environment():
    return {
        **os.environ, "HF_HUB_OFFLINE": "1", "HF_DATASETS_OFFLINE": "1",
        "TRANSFORMERS_OFFLINE": "1", "HF_HUB_DISABLE_TELEMETRY": "1", "WANDB_MODE": "disabled",
        "PYTHONUNBUFFERED": "1", "PYTHONDONTWRITEBYTECODE": "1",
    }


def runtime_environment():
    env = child_environment()
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    return env


def in_lerobot_units(positions, gripper_limits):
    lo, hi = gripper_limits
    opening = (positions[5] - lo) * 100 / (hi - lo)
    return [*(math.degrees(value) for value in positions[:5]), min(100, max(0, opening))]


def in_runtime_units(positions, gripper_limits):
    lo, hi = gripper_limits
    opening = min(1, max(0, positions[5] / 100))
    return [*(math.radians(value) for value in positions[:5]), lo + opening * (hi - lo)]


def score(frames, initial_height):
    cube = [frame["cube_xyz"] for frame in frames]
    end = cube[-1]
    lift = max(position[2] for position in cube) - initial_height
    px, py, pz = frames[-1]["plate_xyz"]
    stable = len(cube) >= SETTLE_FRAMES and all(math.dist(end, xyz) <= 0.008 for xyz in cube[-SETTLE_FRAMES:])
    on_plate = math.hypot(end[0] - px, end[1] - py) <= 0.057 and abs(end[2] - (pz + 0.028)) < 0.011 and stable
    return {
        "cube_peak_lift_m": round(max(0, lift), 4),
        "cube_settled_on_plate": on_plate,
        "cube_final_xyz": end,
        "task_success": lift >= 0.025 and on_plate,
    }
