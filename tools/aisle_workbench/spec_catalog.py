"""Resolve installed Octos app skills and pin their complete package contents."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile

BUILTIN = Path(__file__).resolve().parents[2] / "skills/aisle-nexus-task"
SENSORS = {"G1": {"joint_encoders", "imu"}, "Nano": {"joint_encoders", "onboard_rgb"}}
UNITS = {"G1": {"rad"}, "Nano": {"rad", "m/s", "rad/s"}}


def encode(value):
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def save_atomic(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as f:
        try:
            f.write(encode(value))
            f.close()
            os.replace(f.name, path)
        finally:
            Path(f.name).unlink(missing_ok=True)


def sources():
    registry = os.environ.get("DOROBOT_SPEC_SOURCES")
    extra = json.loads(Path(registry).read_bytes()) if registry else {}
    if not isinstance(extra, dict) or "Studio" in extra:
        raise ValueError(
            "Configure a map of unique package source names. Studio is reserved."
        )
    return {"Studio": BUILTIN, **{key: Path(p) for key, p in extra.items()}}


def describe(root, source, robot, snapshot=None):
    if not root.is_dir():
        raise ValueError(f"Missing installed package from {source}")
    files = {}
    manifest = None
    for p in sorted(root.rglob("*")):
        mode = p.lstat().st_mode
        if stat.S_ISDIR(mode):
            continue
        if not stat.S_ISREG(mode):
            raise ValueError(f"Unsupported package file {p.name}")
        rel = p.relative_to(root).as_posix()
        data = p.read_bytes()
        executable = bool(mode & 0o111)
        files[rel] = {"sha256": digest(data), "executable": executable}
        if rel == "manifest.json":
            manifest = json.loads(data)
        if snapshot:
            dest = snapshot / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
            dest.chmod(0o700 if executable else 0o600)
    if not manifest or not files.get("main", {}).get("executable"):
        raise ValueError(
            "An installed Octos app skill needs manifest.json and an executable main"
        )
    contract = manifest["aisle"]
    profile = contract["profiles"].get(robot)
    if profile is None:
        return None
    if not set(profile["sensors"]) <= SENSORS[robot]:
        raise ValueError(
            f"{source} requests a sensor outside the {robot} observation contract"
        )
    if set(profile["action_units"]) != UNITS[robot]:
        raise ValueError(f"{source} action units differ from the {robot} adapter")
    if contract["verb"] != "vendor.dorobot.aisle.task.run" or "run_task" not in [
        t["name"] for t in manifest["tools"]
    ]:
        raise ValueError("The package does not expose the AISLE task verb")
    return dict(
        source=source,
        name=manifest["name"],
        version=manifest["version"],
        package_sha256=digest(encode(files)),
        main_sha256=files["main"]["sha256"],
        robot=robot,
        sensors=profile["sensors"],
        action_units=profile["action_units"],
        verb=contract["verb"],
    )


def listed(robot):
    entries = []
    errors = []
    for source, root in sources().items():
        try:
            entry = describe(root, source, robot)
            if entry:
                entries.append(entry)
        except (ValueError, KeyError, TypeError, OSError) as exc:
            errors.append(f"{source} · {exc}")
    return entries, errors


def resolved(pin, robot, snapshot=None):
    source = pin["source"]
    root = sources().get(source)
    if root is None:
        raise ValueError(f"Install the selected skill source {source}")
    current = describe(root, source, robot, snapshot)
    if current != pin:
        raise ValueError(
            "The installed skill differs from the selected package. Review it in Task."
        )
    return current


def selected(state, robot):
    path = state / ("selected-" + robot + ".json")
    return json.loads(path.read_bytes()) if path.exists() else None


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("operation", choices=("list", "select", "clear"))
    p.add_argument("--robot", required=True, choices=SENSORS)
    p.add_argument("--state", type=Path, required=True)
    args = p.parse_args()
    file = args.state / ("selected-" + args.robot + ".json")
    if args.operation == "clear":
        file.unlink(missing_ok=True)
    if args.operation == "select":
        import sys

        save_atomic(file, resolved(json.load(sys.stdin), args.robot))
    entries, errors = listed(args.robot)
    print(
        json.dumps(
            dict(
                entries=entries,
                errors=errors,
                selected=selected(args.state, args.robot),
            )
        )
    )


if __name__ == "__main__":
    main()
