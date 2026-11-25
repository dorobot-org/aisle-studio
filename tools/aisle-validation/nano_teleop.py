"""Named, bounded gamepad commands for the simulated Nano; no hardware I/O."""

from __future__ import annotations
import math
import xml.etree.ElementTree as ET
import numpy as np
from nano_living import wheel_rates

MODES = ("Drive", "Left arm", "Right arm", "Head")
TIMEOUT_NS = 250_000_000
AXES = ("left_x", "left_y", "right_x", "right_y")


def model_joints(path):
    joints = [
        j
        for j in ET.parse(path).getroot().findall("joint")
        if j.get("type") not in ("fixed", "floating")
    ]
    names, limits = [], []
    for joint in joints:
        names.append(joint.get("name"))
        limit = joint.find("limit")
        limits.append(
            (-math.inf, math.inf)
            if joint.get("type") == "continuous"
            else (float(limit.get("lower")), float(limit.get("upper")))
        )
    return names, np.asarray(limits)


def axis(value):
    value = float(value)
    if not math.isfinite(value) or abs(value) > 1.01:
        raise ValueError("Invalid controller axis")
    return math.copysign(max(0.0, (abs(value) - 0.12) / 0.88), value)


class TeleopMapper:
    def __init__(self, names):
        self.names = names
        self.mode = 0
        self.paused = False
        self.ready = False
        self.previous = {}
        self.connected = False

    def command(self, sample):
        connected = bool(sample["connected"])
        buttons = sample.get("buttons", {})
        axes = {name: axis(sample.get("axes", {}).get(name, 0)) for name in AXES}
        held = bool(buttons.get("enable"))
        neutral = not any(axes.values()) and not any(
            buttons.get(n) for n in ("up", "down", "left", "right", "open", "close")
        )
        if not connected or not self.connected:
            self.ready = False
        if connected and not held and neutral:
            self.ready = True
        if buttons.get("pause") and not self.previous.get("pause"):
            self.paused = not self.paused
            self.ready = False
        if buttons.get("mode") and not self.previous.get("mode"):
            self.mode = (self.mode + 1) % len(MODES)
            self.ready = False
        enabled = connected and self.ready and held and not self.paused
        velocity = np.zeros(len(self.names))

        def put(name, value):
            velocity[self.names.index(name)] = value

        if enabled:
            if self.mode == 0:
                forward = -axes["left_y"] or (
                    int(bool(buttons.get("up"))) - int(bool(buttons.get("down")))
                )
                turn = -axes["right_x"] or (
                    int(bool(buttons.get("left"))) - int(bool(buttons.get("right")))
                )
                for name, value in zip(
                    ("wheel_1_joint", "wheel_2_joint", "wheel_3_joint"),
                    wheel_rates(forward * 0.12, 0, turn * 0.35),
                ):
                    put(name, value)
            elif self.mode in (1, 2):
                side = "left" if self.mode == 1 else "right"
                values = (
                    axes["left_x"],
                    -axes["left_y"],
                    -axes["right_y"],
                    axes["right_x"],
                    int(bool(buttons.get("right"))) - int(bool(buttons.get("left"))),
                    int(bool(buttons.get("close"))) - int(bool(buttons.get("open"))),
                )
                for suffix, value in zip(
                    (
                        "j1_yaw",
                        "j2_pitch",
                        "j3_pitch",
                        "j4_pitch",
                        "j5_roll",
                        "j6_gripper",
                    ),
                    values,
                ):
                    put(
                        f"{side}_{suffix}",
                        value * (0.5 if suffix == "j6_gripper" else 0.3),
                    )
            else:
                put("head_pan_joint", -axes["left_x"] * 0.35)
                put("head_tilt_joint", -axes["left_y"] * 0.35)
        self.connected = connected
        self.previous = dict(buttons)
        status = (
            "Controller disconnected"
            if not connected
            else "Paused"
            if self.paused
            else "Moving"
            if enabled and np.any(velocity)
            else "L1 held · move a stick or D-pad"
            if enabled
            else "Hold L1 to move"
            if self.ready
            else "Release L1 and center sticks"
        )
        return dict(
            sequence=int(sample["sequence"]),
            input_ns=int(sample["input_ns"]),
            connected=connected,
            enabled=enabled,
            enable_held=held,
            mode=MODES[self.mode],
            status=status,
            velocities=velocity.tolist(),
        )


class CommandGuard:
    def __init__(self, names):
        self.names = names
        self.last = np.zeros(len(names))
        self.last_ns = None
        self.sequence = -1
        self.max_rates = np.array(
            [5.0 if n.startswith("wheel_") else 0.5 for n in names]
        )
        self.acceleration = np.array(
            [25.0 if n.startswith("wheel_") else 3.0 for n in names]
        )

    def accept(self, command, now_ns):
        values = np.asarray(command["velocities"], dtype=float)
        if (
            values.shape != self.last.shape
            or not np.isfinite(values).all()
            or np.any(np.abs(values) > self.max_rates + 1e-9)
        ):
            raise ValueError("Invalid or out-of-range joint command")
        seq = int(command["sequence"])
        if seq <= self.sequence:
            raise ValueError("Reordered controller command")
        self.sequence = seq
        fresh = 0 <= now_ns - int(command["input_ns"]) <= TIMEOUT_NS
        enabled = bool(command["enabled"]) and bool(command["connected"]) and fresh
        dt = (
            0.02
            if self.last_ns is None
            else min(0.1, max(0.0, (now_ns - self.last_ns) * 1e-9))
        )
        self.last = (
            np.clip(
                values,
                self.last - self.acceleration * dt,
                self.last + self.acceleration * dt,
            )
            if enabled
            else np.zeros_like(values)
        )
        self.last_ns = now_ns
        return dict(
            command,
            velocities=self.last.tolist(),
            enabled=enabled,
            status=command["status"] if fresh else "Input timed out",
        )


def active_velocity(command, now_ns, count=17):
    """An independent simulator watchdog, including an input worker crash."""
    if (
        command is None
        or not command.get("enabled")
        or not command.get("connected")
        or not 0 <= now_ns - int(command["input_ns"]) <= TIMEOUT_NS
    ):
        return np.zeros(count)
    values = np.asarray(command["velocities"], dtype=float)
    if values.shape != (count,) or not np.isfinite(values).all():
        raise ValueError("Invalid simulated actuator command")
    return values


def integrate_targets(targets, velocity, limits, dt=0.01):
    return np.clip(
        np.asarray(targets) + np.asarray(velocity) * dt, limits[:, 0], limits[:, 1]
    )
