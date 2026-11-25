"""Dora actors for the supported Nano Feetech base; no AISLE dependency."""
import json
import os
from pathlib import Path
import signal
import sys
import time
import pyarrow as pa
from dora import Node
from feetech import BaseMapper, Bus, Guard, MockSerial, MotorWorker


def write_json(path, value):
    temp = path.with_suffix(".tmp")
    temp.write_text(json.dumps(value, allow_nan=False))
    temp.replace(path)


def send(node, name, value):
    node.send_output(name, pa.array([json.dumps(value, allow_nan=False)]))


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
    try:
        for event in node:
            if event["type"] != "INPUT":
                continue
            if config["self_test"]:
                # Exercise forward, lateral, turn and release through Dora.
                phase = sequence // 40
                sample = dict(connected=True, name="Scripted test controller",
                              axes={"left_y": -.6 if phase == 1 else 0,
                                    "left_x": .6 if phase == 2 else 0,
                                    "right_x": -.6 if phase == 3 else 0},
                              buttons={"enable": 1 <= phase <= 3})
            else:
                pygame.event.pump()
                if pad is not None and not pad.attached():
                    pad.quit()
                    pad = None
                if pad is None:
                    for index in range(controller.get_count()):
                        if controller.is_controller(index):
                            pad = controller.Controller(index)
                            break
                sample = dict(connected=pad is not None, name=pad.name if pad else "No controller",
                              axes={}, buttons={})
                if pad:
                    sample["axes"] = {name: max(-1., min(1., pad.get_axis(code)/32767.))
                                      for name, code in (("left_x", pygame.CONTROLLER_AXIS_LEFTX),
                                                         ("left_y", pygame.CONTROLLER_AXIS_LEFTY),
                                                         ("right_x", pygame.CONTROLLER_AXIS_RIGHTX))}
                    sample["buttons"] = {"enable": bool(pad.get_button(pygame.CONTROLLER_BUTTON_LEFTSHOULDER)),
                                         "pause": bool(pad.get_button(pygame.CONTROLLER_BUTTON_START))}
            sample.update(sequence=sequence, input_ns=time.monotonic_ns())
            send(node, "input", sample)
            if config["self_test"] and sequence == 250:
                (Path(config["output"]) / "self-test-complete").touch()
            sequence += 1
    finally:
        pygame.quit()


def mapping(node, config, guarded=False):
    mapper = Guard() if guarded else BaseMapper()
    for event in node:
        if event["type"] == "INPUT":
            value = json.loads(event["value"][0].as_py())
            result = mapper.accept(value, time.monotonic_ns()) if guarded else mapper.command(value)
            send(node, "safe_command" if guarded else "command", result)


def driver(node, config):
    root = Path(config["output"])
    last = {}
    def publish(value):
        nonlocal last
        last = value
    bus = Bus(MockSerial()) if config["mock"] else Bus.open(config["port"])
    def heartbeat():
        try:
            os.kill(config["supervisor_pid"], 0)
            if config["owner_pid"]:
                os.kill(config["owner_pid"], 0)
                return time.time() - (root / "lease").stat().st_mtime < 1.25
            return True
        except (ProcessLookupError, FileNotFoundError):
            return False
    worker = MotorWorker(bus, publish, config["mock"], heartbeat)
    worker.thread.start()
    try:
        with (root / "status.jsonl").open("a") as history:
            for event in node:
                if event["type"] != "INPUT":
                    continue
                if event["id"] == "command":
                    worker.submit(json.loads(event["value"][0].as_py()))
                else:
                    value = dict(last)
                    if value:
                        write_json(root / "status.json", value)
                        history.write(json.dumps(value) + "\n")
                        history.flush()
                        send(node, "status", value)
                    if worker.error:
                        raise RuntimeError(worker.error)
    finally:
        worker.close()
        write_json(root / "status.json", dict(last))


def main():
    config = json.loads(Path(os.environ["NANO_BASE_CONFIG"]).read_text())
    role = sys.argv[1]
    def terminate(_signum, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, terminate)
    try:
        node = Node()
        if role == "guard":
            mapping(node, config, True)
        elif role == "control":
            mapping(node, config)
        else:
            globals()[role](node, config)
    except KeyboardInterrupt:
        pass
    except Exception as error:
        write_json(Path(config["output"]) / f"failure-{role}.json", {"error": str(error)})
        raise


if __name__ == "__main__":
    main()
