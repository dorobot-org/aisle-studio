"""Run a local Studio Nexus capture, ACT training, or policy rollout job."""

from pathlib import Path
import os
import signal
import sys
import threading
import time
import traceback

import capture
import policy
from workflow_io import JobStopped, Protocol, child_environment, local_path, read_json


def watch_studio(protocol, studio_pid):
    while os.getppid() == studio_pid:
        time.sleep(0.25)
    protocol.stop.set()
    # A Studio-launched worker leads a process group that contains only its job.
    # Reap that group if a blocked native library prevents graceful cancellation.
    time.sleep(5)
    if os.getpgrp() == os.getpid():
        os.killpg(os.getpgrp(), signal.SIGKILL)


def main(config_path):
    config_path = Path(config_path).resolve()
    protocol = Protocol(config_path.parent)
    signal.signal(signal.SIGINT, lambda *_: protocol.stop.set())
    signal.signal(signal.SIGTERM, lambda *_: protocol.stop.set())
    os.environ.update(child_environment())
    try:
        studio_pid = os.environ.get("DOROBOT_NEXUS_STUDIO_PID")
        if studio_pid is not None:
            threading.Thread(
                target=watch_studio, args=(protocol, int(studio_pid)), name="nexus-studio-watch", daemon=True,
            ).start()
        config = read_json(config_path)
        if not isinstance(config, dict) or type(config.get("version")) is not int or config["version"] != 1:
            raise ValueError("Use a Studio Nexus configuration with version 1")
        output = local_path(config.get("output_root", str(config_path.parent)), "output_root").resolve()
        output.mkdir(parents=True, exist_ok=True)
        kind = config.get("kind")
        if kind in ("capture", "rollout"):
            for key in ("runtime", "model", "scene"):
                config[key] = local_path(config.get(key), key)
                if not config[key].is_file():
                    raise ValueError(f"Missing Nexus {key}: {config[key]}")
            if not os.access(config["runtime"], os.X_OK):
                raise ValueError(f"Nexus runtime is not executable: {config['runtime']}")
        protocol.check_stop()
        if kind == "capture":
            capture.run(config, output, protocol)
        elif kind == "train":
            policy.train(config, output, protocol)
        elif kind == "rollout":
            policy.rollout(config, output, protocol)
        else:
            raise ValueError("Job kind must be capture, train, or rollout")
    except JobStopped:
        protocol.update(phase="stopped", message="Local Nexus job stopped")
        return 0
    except Exception as error:
        traceback.print_exc()
        protocol.update(phase="failed", message=str(error) or type(error).__name__)
        return 1
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("Usage: worker.py CONFIG_JSON")
    sys.exit(main(sys.argv[1]))
