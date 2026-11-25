"""Dora gamepad, mapping, guard and physics actors for Nano simulation."""

from __future__ import annotations
import io
import json
import os
import sys
import time
import traceback
from pathlib import Path
import numpy as np
from dora import Node
from nano_living import LivingSimulation
from nano_living_node import send, unpack_info, preview as preview
from nano_living_record import write_json
from nano_session_record import recorder as recorder
from nano_teleop import (
    TeleopMapper,
    CommandGuard,
    active_velocity,
    integrate_targets,
    model_joints,
    TIMEOUT_NS,
)


def gamepad(node, config):
    os.environ["PYGAME_HIDE_SUPPORT_PROMPT"] = "1"
    os.environ["SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS"] = "1"
    import pygame
    from pygame._sdl2 import controller

    pygame.display.init()
    pygame.joystick.init()
    controller.init()
    pad = None
    sequence = 0
    axes = {
        "left_x": pygame.CONTROLLER_AXIS_LEFTX,
        "left_y": pygame.CONTROLLER_AXIS_LEFTY,
        "right_x": pygame.CONTROLLER_AXIS_RIGHTX,
        "right_y": pygame.CONTROLLER_AXIS_RIGHTY,
    }
    buttons = {
        "enable": pygame.CONTROLLER_BUTTON_LEFTSHOULDER,
        "mode": pygame.CONTROLLER_BUTTON_RIGHTSHOULDER,
        "pause": pygame.CONTROLLER_BUTTON_START,
        "open": pygame.CONTROLLER_BUTTON_A,
        "close": pygame.CONTROLLER_BUTTON_B,
        "up": pygame.CONTROLLER_BUTTON_DPAD_UP,
        "down": pygame.CONTROLLER_BUTTON_DPAD_DOWN,
        "left": pygame.CONTROLLER_BUTTON_DPAD_LEFT,
        "right": pygame.CONTROLLER_BUTTON_DPAD_RIGHT,
    }
    try:
        for event in node:
            if event["type"] != "INPUT":
                continue
            pygame.event.pump()
            if pad is not None and not pad.attached():
                pad.quit()
                pad = None
            if pad is None:
                for index in range(controller.get_count()):
                    if controller.is_controller(index):
                        pad = controller.Controller(index)
                        break
            sample = dict(
                sequence=sequence,
                input_ns=time.monotonic_ns(),
                connected=pad is not None,
                name=pad.name if pad else "No controller",
                axes={},
                buttons={},
            )
            if pad is not None:
                sample["axes"] = {
                    name: max(-1.0, min(1.0, pad.get_axis(code) / 32767.0))
                    for name, code in axes.items()
                }
                sample["buttons"] = {
                    name: bool(pad.get_button(code)) for name, code in buttons.items()
                }
            send(node, "input", sample, {})
            if sequence % 10 == 0:
                write_json(Path(config["output"]) / "controller.json", sample)
            sequence += 1
    finally:
        pygame.quit()


def control(node, config):
    names, _ = model_joints(config["model"])
    mapper = TeleopMapper(names)
    for event in node:
        if event["type"] == "INPUT":
            send(node, "command", mapper.command(unpack_info(event)), {})


def guard(node, config):
    names, _ = model_joints(config["model"])
    gate = CommandGuard(names)
    for event in node:
        if event["type"] == "INPUT":
            command = gate.accept(unpack_info(event), time.monotonic_ns())
            send(node, "safe_command", command, {"guard_pid": os.getpid()})
            if command["sequence"] % 10 == 0:
                write_json(Path(config["output"]) / "command.json", command)


def sim(node, config):
    import cv2

    root = Path(config["output"])
    write_json(
        root / "startup.json",
        {
            "message": "Preparing physics and 3 cameras. First launch compiles GPU shaders; this may take a few minutes."
        },
    )
    simulation = LivingSimulation(
        Path(config["model"]), cameras=True, resolution=(640, 480)
    )
    robot_bodies = {
        simulation.scene.gpu_index(0, b) for b in simulation.robot.link_bodies[0]
    }
    furniture = {
        simulation.scene.gpu_index(0, b.handles[0])
        for b, spec in zip(simulation.bodies, simulation.specs)
        if spec["fixed"] and spec["name"] != "floor"
    }
    cube_index = next(
        i for i, spec in enumerate(simulation.specs) if spec["name"] == "cube_6"
    )
    cube_body = simulation.scene.gpu_index(0, simulation.bodies[cube_index].handles[0])
    for spec in simulation.camera_specs.values():
        spec["fps"] = 5
    names, limits = model_joints(config["model"])
    if names != simulation.names:
        raise ValueError("Nano joint order mismatch")
    info = dict(
        version=1,
        session="Nano gamepad · " + config["run_id"],
        fps=5,
        joint_names=names,
        objects=[
            {k: s[k] for k in ("name", "size", "color")} for s in simulation.specs
        ],
        cameras=simulation.camera_specs,
        recording=False,
        physics_dt=0.01,
        requested_control_hz=50,
        mode="simulation",
        controller="PS4 / PS5 via SDL",
        studio_session_version=1,
        contact_audit_hz=100,
        initial_objects=simulation.cube_start,
        model=config["model"],
    )
    send(node, "scene_info", info, {})
    write_json(root / "scene-info.json", info)
    command = None
    sequence = 0
    started = time.monotonic()
    next_preview = 20
    armed_before = False
    rearm_required = False
    total_command_age = 0.0
    samples = 0
    request_id = None
    recording = False
    recording_sequence = 0
    contact_samples = []
    for event in node:
        if event["type"] != "INPUT":
            continue
        if event["id"] == "command":
            if "guard_pid" not in event["metadata"]:
                raise ValueError("Unguarded teleoperation command")
            candidate = unpack_info(event)
            if command is None or candidate["sequence"] > command["sequence"]:
                command = candidate
            continue
        if event["id"] != "tick":
            continue
        control_file = root / "session-command.json"
        try:
            request = json.loads(control_file.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            request = {}
        if request.get("id") is not None and request["id"] != request_id:
            request_id = request["id"]
            operation = request.get("action")
            if operation == "record" and not recording:
                recording = True
                recording_sequence = 0
                send(
                    node,
                    "recording",
                    dict(info=info, request=request),
                    {"operation": "start"},
                )
            elif operation == "stop" and recording:
                recording = False
                send(
                    node,
                    "recording",
                    dict(request, frame_count=recording_sequence),
                    {"operation": "stop"},
                )
            elif operation in ("save", "discard") and not recording:
                send(node, "recording", request, {"operation": operation})
        now_ns = time.monotonic_ns()
        velocity = active_velocity(command, now_ns)
        fresh = command is not None and 0 <= now_ns - command["input_ns"] <= TIMEOUT_NS
        if armed_before and not fresh:
            rearm_required = True
        if fresh and not command.get("enable_held", True):
            rearm_required = False
        armed = fresh and command["enabled"] and not rearm_required
        if not armed:
            velocity[:] = 0
        if armed_before and not armed:
            # Hold the measured pose on release; do not chase an accumulated
            # target after a collision or a lost controller connection.
            simulation.targets = simulation.observation()[7:24].copy()
        armed_before = armed
        for _ in range(2):
            simulation.targets = integrate_targets(simulation.targets, velocity, limits)
            simulation.robot.control_dofs_position(
                simulation.targets.tolist(), simulation.dofs
            )
            simulation.scene.step()
            simulation.tick += 1
            if recording:
                for contact in simulation.scene.state.debug_contacts(
                    simulation.engine.viewer
                ):
                    contact_samples.append(
                        [
                            simulation.tick,
                            int(contact["body_a"]),
                            int(contact["body_b"]),
                        ]
                    )
        if fresh:
            total_command_age += (now_ns - command["input_ns"]) * 1e-6
            samples += 1
        if simulation.tick < next_preview:
            continue
        next_preview += 20
        obs = simulation.observation()
        snap = simulation.snapshot()
        payload = dict(
            observation=obs, transforms=np.asarray(snap["transforms"], np.float32)
        )
        try:
            if (
                json.loads((root / "recording-status.json").read_text())["phase"]
                == "error"
            ):
                recording = False
        except (FileNotFoundError, json.JSONDecodeError):
            pass
        if recording:
            from nano_cameras import read_rig

            stamp = (recording_sequence + 1) * 200_000_000
            bad = [
                [a, b]
                for _, a, b in contact_samples
                if (a in robot_bodies and b in furniture)
                or (b in robot_bodies and a in furniture)
            ]
            touched = any(
                (a in robot_bodies and b == cube_body)
                or (b in robot_bodies and a == cube_body)
                for _, a, b in contact_samples
            )
            recorded = dict(
                payload,
                contacts=np.asarray(bad, dtype=np.int64).reshape(-1, 2),
                physics_contacts=np.asarray(contact_samples, dtype=np.int64).reshape(
                    -1, 3
                ),
                cube_contact=np.asarray([int(touched)], dtype=np.int64),
                command_velocity=velocity,
                source_sim_time_ns=np.int64(simulation.tick * 10_000_000),
            )
            for name, arrays in read_rig(
                simulation.rig, simulation.camera_specs, stamp
            ):
                recorded.update(
                    {name + "__" + key: value for key, value in arrays.items()}
                )
            raw = io.BytesIO()
            np.savez(raw, **recorded)
            send(
                node,
                "recording",
                raw.getvalue(),
                dict(
                    operation="frame",
                    sequence=recording_sequence,
                    sim_time_ns=stamp,
                    phase="Manual teleoperation",
                    goal_index=0,
                ),
            )
            recording_sequence += 1
        contact_samples.clear()
        for name, camera in simulation.rig.items():
            if recording:
                rgb = recorded[name + "__rgb"]
            else:
                rgb, _, _, _ = camera.render(rgb=True, depth=False, segmentation=False)
            ok, jpeg = cv2.imencode(
                ".jpg",
                cv2.cvtColor(np.asarray(rgb, np.uint8), cv2.COLOR_RGB2BGR),
                [cv2.IMWRITE_JPEG_QUALITY, 90],
            )
            if not ok:
                raise RuntimeError(f"Camera encode failed: {name}")
            payload[name + "__jpeg"] = jpeg
        buffer = io.BytesIO()
        np.savez(buffer, **payload)
        mode = command["mode"] if command else "Drive"
        status = (
            command["status"]
            if fresh and not rearm_required
            else "Input timed out · release L1"
        )
        phase = f"{'Recording' if recording else 'Preview only'} · Gamepad · {mode} · {status}"
        send(
            node,
            "preview_frame",
            buffer.getvalue(),
            {
                "sequence": sequence,
                "sim_time_ns": simulation.tick * 10_000_000,
                "phase": phase,
            },
        )
        elapsed = time.monotonic() - started
        write_json(
            root / "progress.json",
            dict(
                frame=sequence,
                sim_time=simulation.tick * 0.01,
                wall_seconds=elapsed,
                physics_hz=simulation.tick / max(elapsed, 0.001),
                preview_hz=(sequence + 1) / max(elapsed, 0.001),
                mean_command_age_ms=total_command_age / max(samples, 1),
                phase=phase,
                mode=mode,
                enabled=bool(armed),
                controller_fresh=bool(fresh),
                input_sequence=command["sequence"] if command else None,
                base=obs[:7].tolist(),
                joints=obs[7:24].tolist(),
                targets=simulation.targets.tolist(),
                velocity=velocity.tolist(),
                recording=recording,
                root_pose_writes=simulation.root_pose_writes,
            ),
        )
        sequence += 1


def main():
    config = json.loads(Path(os.environ["NANO_LIVING_CONFIG"]).read_text())
    role = sys.argv[1]
    root = Path(config["output"])
    write_json(root / f"node-{role}.json", {"role": role, "pid": os.getpid()})
    try:
        globals()[role](Node(), config)
    except BaseException as error:
        write_json(
            root / f"failure-{role}.json",
            {"error": str(error), "traceback": traceback.format_exc()},
        )
        raise


if __name__ == "__main__":
    main()
