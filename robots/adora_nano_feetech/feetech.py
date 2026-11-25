"""Persistent Feetech bus, adapted from the supplied omni-car bridge protocol.

Speeds are vendor ticks, not SI velocities. No estimated joint/odometry feedback
is published as a measurement. Every unicast write requires a valid motor ACK.
"""
from __future__ import annotations
import json
import math
import threading
import time
from pathlib import Path

PROFILE = json.loads(Path(__file__).with_name("profile.json").read_text())
IDS = tuple(PROFILE["wheel_ids"])
REG = PROFILE["registers"]
LIMIT = PROFILE["teleop_limit_ticks"]
TIMEOUT_NS = PROFILE["command_timeout_ms"] * 1_000_000


def packet(motor, instruction, params=()):
    body = bytes([motor, len(params) + 2, instruction, *params])
    return b"\xff\xff" + body + bytes([(~sum(body)) & 255])


def speed_word(value):
    if not math.isfinite(value) or abs(value) > PROFILE["speed_limit_ticks"]:
        raise ValueError("Invalid wheel speed")
    return int(abs(value)) | (0x8000 if value < 0 else 0)


def mix(vx, vy, omega, limit=LIMIT):
    if not all(math.isfinite(v) for v in (vx, vy, omega)):
        raise ValueError("Non-finite base command")
    # Preserve the provided driver's signs and ID order; do not substitute
    # the simulation's wheel directions or its uncalibrated SI gains.
    wheels = [-0.866 * vx + 0.5 * vy + omega, -vy + omega,
              0.866 * vx + 0.5 * vy + omega]
    scale = max(1.0, max(map(abs, wheels)) / limit)
    return [v / scale for v in wheels]


class MockSerial:
    """Byte-level bus emulator: exercises the same framing and ACK validator."""
    def __init__(self):
        self.rx = bytearray()
        self.writes = []
        self.registers = {}
        self.fault = None

    def reset_input_buffer(self):
        self.rx.clear()

    def write(self, data):
        assert data[:2] == b"\xff\xff" and sum(data[2:]) & 255 == 255
        assert len(data) == data[3] + 4 and data[4] == 3
        motor, address = data[2], data[5]
        self.writes.append(bytes(data))
        self.registers[motor, address] = int.from_bytes(data[6:-1], "little")
        response = packet(motor, 0)
        if self.fault == "checksum":
            response = response[:-1] + bytes([response[-1] ^ 1])
        elif self.fault == "wrong_id":
            response = packet(motor + 1, 0)
        elif self.fault == "motor_error":
            response = packet(motor, 4)
        elif self.fault == "timeout":
            response = b""
        self.rx.extend(response)
        return len(data)

    def read(self, count):
        out = bytes(self.rx[:count])
        del self.rx[:count]
        return out

    def close(self):
        pass


class Bus:
    def __init__(self, serial):
        self.serial = serial
        self.acks = 0
        self.torque = False

    @classmethod
    def open(cls, port):
        import serial
        return cls(serial.Serial(port, PROFILE["baud"], timeout=0.02,
                                 write_timeout=0.02, exclusive=True))

    def write(self, motor, name, value, width=1):
        request = packet(motor, 3, [REG[name], *value.to_bytes(width, "little")])
        self.serial.reset_input_buffer()
        if self.serial.write(request) != len(request):
            raise OSError(f"Incomplete write to motor {motor}")
        deadline = time.monotonic() + 0.025
        response = bytearray()
        while len(response) < 6 and time.monotonic() < deadline:
            response.extend(self.serial.read(6 - len(response)))
        if len(response) != 6:
            raise OSError(f"Motor {motor}: acknowledgement timed out")
        if response[:4] != bytes([255, 255, motor, 2]) or sum(response[2:]) & 255 != 255:
            raise OSError(f"Motor {motor}: invalid acknowledgement")
        if response[4]:
            raise OSError(f"Motor {motor}: error status 0x{response[4]:02x}")
        self.acks += 1

    def prepare(self):
        # Zero and disable all wheels before mode changes or torque enable.
        errors = self.stop()
        if errors:
            raise OSError("; ".join(errors))
        for motor in IDS:
            self.write(motor, "lock", 0)
            self.write(motor, "mode", 1)
            self.write(motor, "lock", 1)

    def drive(self, speeds, enabled):
        if len(speeds) != 3:
            raise ValueError("Expected three wheels")
        words = [speed_word(v) for v in speeds]
        if enabled and not self.torque:
            for motor in IDS:
                self.write(motor, "speed", 0, 2)
            for motor in IDS:
                self.write(motor, "torque", 1)
            self.torque = True
        for motor, word in zip(IDS, words if enabled else [0, 0, 0]):
            self.write(motor, "speed", word, 2)
        if not enabled and self.torque:
            for motor in IDS:
                self.write(motor, "torque", 0)
            self.torque = False

    def stop(self):
        errors = []
        # Attempt every wheel even if one motor has failed. Never claim success
        # when a zero-speed or torque-disable command wasn't acknowledged.
        for name, width in (("speed", 2), ("torque", 1)):
            for motor in IDS:
                try:
                    self.write(motor, name, 0, width)
                except Exception as error:
                    errors.append(str(error))
        self.torque = False
        return errors


class BaseMapper:
    def __init__(self):
        self.ready = False
        self.connected = False
        self.paused = False
        self.pause_was_held = False

    def command(self, sample):
        connected = sample["connected"] is True
        axes = []
        for name in ("left_x", "left_y", "right_x"):
            value = float(sample.get("axes", {}).get(name, 0))
            if not math.isfinite(value) or abs(value) > 1.01:
                raise ValueError("Invalid gamepad axis")
            axes.append(math.copysign(max(0, (abs(value) - .12) / .88), value))
        buttons = sample.get("buttons", {})
        held = buttons.get("enable") is True
        pause = buttons.get("pause") is True
        neutral = not any(axes)
        if not connected or not self.connected:
            self.ready = False
        if connected and not held and neutral:
            self.ready = True
        if pause and not self.pause_was_held:
            self.paused = not self.paused
            self.ready = False
        enabled = connected and self.ready and held and not self.paused
        self.connected, self.pause_was_held = connected, pause
        wheels = mix(-axes[1] * LIMIT, -axes[0] * LIMIT, -axes[2] * LIMIT)
        return dict(sequence=sample["sequence"], input_ns=sample["input_ns"],
                    connected=connected, enabled=enabled, enable_held=held,
                    neutral=neutral, wheels=wheels if enabled else [0., 0., 0.],
                    controller=sample.get("name", "Controller"),
                    status="Controller disconnected" if not connected else
                    "Paused" if self.paused else "Moving" if enabled and any(wheels) else
                    "L1 held · move sticks" if enabled else
                    "Hold L1 to drive" if self.ready else "Release L1 and center sticks")


class Guard:
    def __init__(self):
        self.sequence = -1
        self.last_ns = None
        self.last = [0., 0., 0.]

    def accept(self, command, now):
        values = command["wheels"]
        if (len(values) != 3 or any(not math.isfinite(v) or abs(v) > LIMIT + 1e-6 for v in values)
                or type(command["sequence"]) is not int or command["sequence"] <= self.sequence
                or type(command["input_ns"]) is not int):
            raise ValueError("Invalid or reordered base command")
        self.sequence = command["sequence"]
        enabled = (command["enabled"] is True and command["connected"] is True
                   and command["enable_held"] is True and 0 <= now-command["input_ns"] <= TIMEOUT_NS)
        dt = .02 if self.last_ns is None else max(0, min(.1, (now-self.last_ns)*1e-9))
        self.last = [max(old-600*dt, min(old+600*dt, v)) for old, v in zip(self.last, values)] if enabled else [0., 0., 0.]
        self.last_ns = now
        return dict(command, enabled=enabled, wheels=list(self.last))


class MotorWorker:
    """Serial owner with a watchdog independent of the Dora receive iterator."""
    def __init__(self, bus, publish, mock=False, heartbeat=lambda: True):
        self.bus, self.publish, self.mock = bus, publish, mock
        self.heartbeat = heartbeat
        self.lock = threading.Lock()
        self.command = None
        self.sequence = -1
        self.rearm = True
        self.shutdown = threading.Event()
        self.thread = threading.Thread(target=self.run, name="feetech-watchdog")
        self.error = None

    def submit(self, command):
        with self.lock:
            if command["sequence"] <= self.sequence:
                raise ValueError("Reordered driver command")
            if len(command["wheels"]) != 3 or any(not math.isfinite(v) or abs(v) > LIMIT + 1e-6 for v in command["wheels"]):
                raise ValueError("Invalid driver speed")
            self.sequence = command["sequence"]
            self.command = dict(command)

    def step(self, now):
        with self.lock:
            c = self.command
        fresh = c is not None and 0 <= now-c["input_ns"] <= TIMEOUT_NS
        if not fresh or not c["connected"]:
            self.rearm = True
        if fresh and c["connected"] and not c["enable_held"] and c["neutral"]:
            self.rearm = False
        enabled = bool(fresh and c["enabled"] and c["enable_held"] and c["connected"] and not self.rearm)
        speeds = c["wheels"] if enabled else [0., 0., 0.]
        self.bus.drive(speeds, enabled)
        return dict(phase="ready", mock=self.mock, enabled=enabled,
                    status="Input timed out · release L1" if not fresh else
                    "Release L1 and center sticks" if self.rearm else c["status"],
                    controller=c.get("controller", "") if c else "",
                    command_ticks=[int(v) for v in speeds], motor_ids=list(IDS),
                    acks=self.bus.acks, input_age_ms=(now-c["input_ns"])/1e6 if c else None,
                    measured_feedback=False, updated_ns=time.time_ns())

    def run(self):
        try:
            self.bus.prepare()
            while not self.shutdown.is_set():
                if not self.heartbeat():
                    raise RuntimeError("Studio or supervisor heartbeat lost")
                self.publish(self.step(time.monotonic_ns()))
                self.shutdown.wait(.02)
        except Exception as error:
            self.error = str(error)
        finally:
            errors = self.bus.stop()
            if errors:
                self.error = "; ".join(filter(None, [self.error, *errors]))
            self.publish(dict(phase="fault" if self.error else "stopped", mock=self.mock,
                              enabled=False, status=self.error or "Stopped · torque disabled",
                              stop_confirmed=not errors, command_ticks=[0, 0, 0],
                              acks=self.bus.acks, updated_ns=time.time_ns()))
            self.bus.serial.close()

    def close(self):
        self.shutdown.set()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise RuntimeError("Serial worker did not stop")
