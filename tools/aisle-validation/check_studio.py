"""Run the Nano fixture through Studio's production backend and video decoder."""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", type=Path)
    parser.add_argument(
        "--studio",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    args = parser.parse_args()
    dataset, studio = args.dataset.resolve(), args.studio.resolve()
    example = studio / "dorobot-flex/examples/check_nano_aisle.rs"
    if example.exists():
        parser.error(f"Refusing to overwrite {example}")
    log_path = dataset.parent / "studio-integration.log"
    with tempfile.TemporaryDirectory(prefix="nano-studio-target-") as target:
        shutil.copy2(Path(__file__).with_name("check_nano_aisle.rs"), example)
        try:
            env = dict(
                os.environ,
                CARGO_TARGET_DIR=target,
                CARGO_INCREMENTAL="0",
                CARGO_PROFILE_DEV_DEBUG="0",
            )
            with log_path.open("w") as log:
                process = subprocess.run(
                    [
                        "cargo",
                        "run",
                        "--locked",
                        "-p",
                        "dorobot-flex",
                        "--example",
                        "check_nano_aisle",
                        "--",
                        str(dataset),
                    ],
                    cwd=studio,
                    env=env,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                )
        finally:
            example.unlink()
    if process.returncode:
        print(
            json.dumps(
                {"ok": False, "log": str(log_path), "exit_code": process.returncode}
            )
        )
        return process.returncode
    report = next(
        json.loads(line)
        for line in reversed(log_path.read_text().splitlines())
        if line.startswith('{"action_channels"')
    )
    report["studio_commit"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=studio, text=True
    ).strip()
    (dataset.parent / "studio-verification.json").write_text(
        json.dumps(report, indent=2) + "\n"
    )
    print(json.dumps(report))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
