"""Exercise actual encoding/export and recoverable save failures with synthetic sensors."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import cv2
import numpy as np
import pyarrow.parquet as pq
from nano_session_record import SessionRecorder
from nano_living import CAMERAS


class RecordingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.session = SessionRecorder(dict(output=str(self.root / "session")))
        self.info = dict(
            fps=5,
            joint_names=[f"joint{i}" for i in range(17)],
            objects=[dict(name="cube_6", size=[0.1] * 3, color=[1] * 4)],
            initial_objects=[[0, 0, 0]],
        )

    def tearDown(self):
        if self.session.writer:
            self.session.fail("test teardown")
        self.temp.cleanup()

    def record(self, identity=1):
        self.session.start(self.info, dict(id=identity, task="Place cube"))
        for i in range(3):
            stamp = (i + 1) * 200_000_000
            data = dict(
                observation=np.arange(41, dtype=np.float32) + i * 0.01,
                transforms=np.eye(4, dtype=np.float32).reshape(1, 16),
                command_velocity=np.zeros(17),
                contacts=np.zeros((0, 2)),
                cube_contact=np.zeros(1),
                source_sim_time_ns=np.int64(stamp + 4_000_000_000),
            )
            for name in CAMERAS:
                data.update(
                    {
                        name + "__rgb": np.full((480, 640, 3), i * 60, dtype=np.uint8),
                        name + "__depth": np.ones((480, 640), np.float32),
                        name + "__seg": np.ones((480, 640), np.int32),
                        name + "__sim_time_ns": np.int64(stamp),
                    }
                )
            self.session.append(
                data, dict(sequence=i, sim_time_ns=stamp, phase="Manual", goal_index=0)
            )
        self.session.stop()

    def test_three_camera_export_and_second_episode(self):
        for episode in (1, 2):
            self.record(episode)
            self.assertEqual(self.session.status["phase"], "review")
            self.session.save(dict(destination=str(self.root / "saved")))
            dataset = Path(self.session.status["dataset"])
            info = json.loads((dataset / "meta/info.json").read_text())
            self.assertEqual(info["total_frames"], 3)
            capture = json.loads((dataset / "meta/capture.json").read_text())
            self.assertEqual(capture["model"], "adora_nano.urdf")
            self.assertIn("source_sim_time_ns", capture["clock"])
            videos = list(dataset.glob("videos/**/*.mp4"))
            self.assertEqual(len(videos), 3)
            for path in videos:
                reader = cv2.VideoCapture(str(path))
                self.assertEqual(int(reader.get(cv2.CAP_PROP_FRAME_COUNT)), 3)
                reader.release()
            table = pq.read_table(next(dataset.glob("data/**/*.parquet")))
            self.assertEqual(len(table["observation.state"][0].as_py()), 17)
            raw = list((dataset / "raw").glob("frame_*.npz"))
            self.assertEqual(len(raw), 3)
            with np.load(raw[0]) as frame:
                self.assertTrue(all(name + "__depth" in frame for name in CAMERAS))
                self.assertIn("source_sim_time_ns", frame)
        self.assertEqual(self.session.saved, 2)

    def test_failed_save_retains_take_and_can_retry(self):
        self.record()
        with patch(
            "nano_session_record.shutil.copytree", side_effect=OSError("disk full")
        ):
            with self.assertRaises(OSError) as failure:
                self.session.save(dict(destination=str(self.root / "saved")))
        self.session.fail(failure.exception)
        self.assertEqual(self.session.status["phase"], "review")
        self.assertTrue(self.session.take.exists())
        self.session.save(dict(destination=str(self.root / "saved")))
        self.assertTrue(Path(self.session.status["dataset"]).exists())

    def test_pending_take_blocks_new_capture_and_discard_clears_it(self):
        self.record()
        with self.assertRaisesRegex(ValueError, "pending episode"):
            self.session.start(self.info, dict(id=2, task="Next"))
        self.session.discard()
        self.assertEqual(self.session.status["phase"], "idle")
        self.assertFalse(list((self.root / "session/takes").iterdir()))

    def test_stop_waits_for_late_sensor_frame_before_save(self):
        from nano_session_record import recorder

        self.record()
        frames = []
        for path in sorted((self.session.take / "raw").glob("frame_*.npz")):
            with np.load(path) as raw:
                frames.append(
                    {
                        k: raw[k]
                        for k in raw.files
                        if k not in ("command", "proposed", "sim_time_ns")
                    }
                )
        root = self.root / "reordered"

        def event(operation, value, **metadata):
            return dict(
                type="INPUT",
                id="recording",
                value=value,
                metadata=dict(operation=operation, **metadata),
            )

        events = [
            event("start", dict(info=self.info, request=dict(id=10, task="Task")))
        ]
        for index in [0, 2]:
            events.append(
                event(
                    "frame",
                    frames[index],
                    sequence=index,
                    sim_time_ns=(index + 1) * 200_000_000,
                    phase="Manual",
                    goal_index=0,
                )
            )
        events += [
            event("stop", dict(id=11, frame_count=3)),
            event(
                "frame",
                frames[1],
                sequence=1,
                sim_time_ns=400_000_000,
                phase="Manual",
                goal_index=0,
            ),
            event("save", dict(id=12, destination=str(self.root / "late-save"))),
        ]
        with (
            patch("nano_living_node.unpack_info", side_effect=lambda e: e["value"]),
            patch("nano_living_node.arrays", side_effect=lambda e: e["value"]),
        ):
            recorder(events, dict(output=str(root)))
        status = json.loads((root / "recording-status.json").read_text())
        self.assertEqual(status["phase"], "saved")
        self.assertEqual(status["frames"], 3)
        self.assertEqual(status["request_id"], 12)
        self.assertFalse((root / "recording-errors.log").exists())

    def test_empty_capture_is_actionable(self):
        self.session.start(self.info, dict(id=1, task="Task"))
        self.session.stop()
        self.assertEqual(self.session.status["phase"], "error")
        self.assertIn("No frames", self.session.status["message"])
        self.session.discard()


if __name__ == "__main__":
    unittest.main()
