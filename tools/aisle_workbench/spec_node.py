"""Tie one Dora-owned process group to the lifetime of its Studio run."""

import fcntl
import json
import os
from pathlib import Path
import runpy
import shutil
import signal
import sys


def main():
    role = sys.argv[1]
    root = Path(os.environ["AISLE_SPEC_JOB"]).parent
    job = json.loads((root / "job.json").read_bytes())
    output = Path(job["args"]["out"]["local_path"])
    pid = os.getpid()
    if os.getpgrp() != pid:
        raise RuntimeError(
            "Dora must launch the simulation node in its own process group"
        )
    # Open before forking. A late-starting node must stop even if the owner is
    # already gone. Each guard belongs to, and can only terminate, its own group.
    with (root / "owner.lock").open("rb") as lock:
        if os.fork() == 0:
            try:
                for fd in (0, 1, 2):
                    os.close(fd)
                fcntl.flock(lock, fcntl.LOCK_SH)
                if role == "vendor" and not (root / "owner-finishing").exists():
                    shutil.rmtree(root, ignore_errors=True)
            finally:
                os.killpg(pid, signal.SIGKILL)
                os._exit(0)
    (output / ("spec-" + role + "-pid.json")).write_text(json.dumps({"pid": pid}))
    if role == "vendor":
        runpy.run_module("spec_runtime", run_name="__main__")
    elif role == "bridge":
        runpy.run_module("octos_spec_bridge", run_name="__main__")
    else:
        raise ValueError("Unknown SPEC node role")


if __name__ == "__main__":
    main()
