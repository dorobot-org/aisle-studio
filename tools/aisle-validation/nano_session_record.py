"""Dora recording actor for explicit Studio start/stop/save/discard commands."""

import json
import shutil
import time
import traceback
from pathlib import Path

from nano_living_record import LivingRecorder, write_json


class SessionRecorder:
    def __init__(self, config):
        self.config = config
        self.root = Path(config["output"])
        self.writer = None
        self.take = None
        self.saved = 0
        self.status = dict(phase="idle", frames=0, saved=0, message="Ready to record.")
        self.publish()

    def publish(self, **values):
        self.status.update(values, saved=self.saved)
        write_json(self.root / "recording-status.json", self.status)

    def start(self, info, request):
        if self.writer or self.take:
            raise ValueError(
                "Save or discard the pending episode before recording again."
            )
        task = str(request.get("task", "")).strip()
        if not task:
            raise ValueError("Enter a task description before recording.")
        self.take = self.root / "takes" / str(int(request["id"]))
        self.take.mkdir(parents=True, exist_ok=False)
        config = dict(
            self.config,
            output=str(self.take),
            width=640,
            height=480,
            task=task,
            manual_recording=True,
        )
        self.writer = LivingRecorder(config, info)
        self.publish(
            phase="recording",
            frames=0,
            task=task,
            message="Recording 3 cameras, 17 joints and scene motion.",
        )

    def append(self, data, meta):
        if not self.writer:
            raise ValueError("Recording frame without a started episode.")
        self.writer.append(
            data,
            meta,
            data["command_velocity"][:3].tolist(),
            data["command_velocity"][:3].tolist(),
        )
        self.publish(frames=self.writer.expected)

    def stop(self):
        if not self.writer:
            return
        if not self.writer.expected:
            for writer in self.writer.writers.values():
                writer.release()
            self.writer.audit.close()
            self.writer = None
            self.publish(
                phase="error",
                message="No frames captured. Discard this empty take and try again.",
            )
            return
        self.publish(
            phase="saving", message="Finishing videos and checking captured frames…"
        )
        result = self.writer.finish(False)
        self.writer = None
        self.publish(
            phase="review",
            frames=result["frames"],
            staged=str(self.take),
            message="Episode ready. Save it to keep it, or discard it.",
        )

    def save(self, request):
        if self.writer or not self.take or self.status["phase"] != "review":
            raise ValueError("Stop recording before saving the episode.")
        destination = Path(request["destination"]).expanduser()
        if not destination.is_absolute():
            raise ValueError("Choose an absolute recording folder.")
        destination.mkdir(parents=True, exist_ok=True)
        target = destination / ("nano-" + self.take.name)
        if target.exists():
            raise ValueError(
                "The destination episode already exists; choose another folder."
            )
        dataset = self.take / "lerobot"
        raw = self.take / "raw"
        scene = json.loads((self.take / "scene-info.json").read_text())
        scene["model"] = Path(scene.get("model", "adora_nano.urdf")).name
        scene["clock"] = (
            "Episode stamps start at one capture interval; source_sim_time_ns retains the scene clock in each raw frame."
        )
        scene["raw_layout"] = (
            "raw/frame_XXXXXX.npz; camera arrays use camera-name__field keys"
        )
        write_json(dataset / "meta/capture.json", scene)
        summary = json.loads((self.take / "result.json").read_text())
        summary.pop("dataset", None)
        summary.pop("raw", None)
        write_json(dataset / "meta/capture-result.json", summary)
        if raw.exists():
            shutil.move(str(raw), str(dataset / "raw"))
        # An in-progress destination is not discoverable as a dataset.
        staging = destination / (".nano-" + self.take.name + ".saving")
        if staging.exists():
            raise ValueError(
                "An unfinished save exists. Choose another recording folder."
            )
        self.publish(phase="saving", message="Saving episode and raw sensor data…")
        try:
            shutil.copytree(dataset, staging)
            staging.rename(target)
        except Exception:
            if staging.exists():
                shutil.rmtree(staging)
            self.publish(phase="review")
            raise
        self.saved += 1
        self.publish(
            phase="saved",
            dataset=str(target),
            staged=None,
            message="Episode saved. Open Replay or record the next episode.",
        )
        shutil.rmtree(self.take)
        self.take = None

    def discard(self):
        if self.writer:
            raise ValueError("Stop recording before discarding the episode.")
        if self.take:
            shutil.rmtree(self.take)
        self.take = None
        self.publish(
            phase="idle",
            frames=0,
            staged=None,
            message="Take discarded. Ready to record.",
        )

    def fail(self, error):
        if self.writer:
            for writer in self.writer.writers.values():
                writer.release()
            self.writer.audit.close()
            self.writer = None
        phase = (
            "review"
            if self.take and (self.take / "lerobot/meta/info.json").is_file()
            else "error"
        )
        self.publish(
            phase=phase,
            staged=str(self.take) if self.take else None,
            message=f"{error} Captured data retained; retry Save or discard the take.",
        )


def recorder(node, config):
    from nano_living_node import arrays, unpack_info

    session = SessionRecorder(config)
    frames = {}
    pending_stop = None
    stop_started = 0
    for event in node:
        if event["type"] != "INPUT":
            continue
        if event["id"] == "tick":
            if pending_stop and time.monotonic() - stop_started > 15:
                session.fail(
                    RuntimeError(
                        "Capture did not receive every frame. Raw data retained; discard and retry."
                    )
                )
                session.publish(request_id=pending_stop["id"])
                pending_stop = None
                frames.clear()
            continue
        operation = event["metadata"].get("operation")
        try:
            if operation == "start":
                request = unpack_info(event)
                session.start(request["info"], request["request"])
                session.publish(request_id=request["request"]["id"])
            elif operation == "frame":
                sequence = int(event["metadata"]["sequence"])
                if sequence in frames or (
                    session.writer and sequence < session.writer.expected
                ):
                    raise ValueError("Duplicate recording frame")
                frames[sequence] = (arrays(event), event["metadata"])
                if len(frames) > 64:
                    raise ValueError("Recording cannot keep up; raw data retained.")
            elif operation == "stop":
                pending_stop = unpack_info(event)
                stop_started = time.monotonic()
            elif operation in ("save", "discard"):
                request = unpack_info(event)
                if operation == "save":
                    session.save(request)
                else:
                    session.discard()
                    frames.clear()
                session.publish(request_id=request["id"])
            # Dora may deliver the small stop message ahead of the last large
            # sensor frame. Commit only after the producer's complete count.
            while session.writer and session.writer.expected in frames:
                data, meta = frames.pop(session.writer.expected)
                session.append(data, meta)
            if (
                pending_stop
                and session.writer
                and session.writer.expected == pending_stop["frame_count"]
            ):
                session.stop()
                session.publish(request_id=pending_stop["id"])
                pending_stop = None
        except Exception as error:
            with (session.root / "recording-errors.log").open("a") as log:
                log.write(traceback.format_exc())
            session.fail(error)
            if operation != "frame":
                payload = unpack_info(event)
                session.publish(request_id=payload.get("request", payload).get("id"))
    if session.writer:
        session.fail(
            RuntimeError("Simulation ended during capture. Raw data retained.")
        )
