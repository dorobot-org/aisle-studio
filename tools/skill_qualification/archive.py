"""Export or reopen the evidence needed to replay a complete skill qualification."""

import argparse
import os
from pathlib import Path
import re
import shutil
import stat
from tempfile import NamedTemporaryFile, TemporaryDirectory
from zipfile import ZIP_DEFLATED, ZipFile

import cohort
import review

SHARED = {
    "controller.json",
    "suite.json",
    "registration.json",
    "verification.json",
    "qualification-rows.json",
    "reviews.json",
}
NANO = {
    "result.json",
    "source.json",
    "calibration.json",
    "actions.jsonl",
    "controller.json",
    "motor-targets.f64",
}
SPEC = {"spec-measurement.json", "spec-receipt.json"}
MAX_FILE = 32 * 1024**2
MAX_TOTAL = 1024**3


def allowed(path, robot, count):
    if path in SHARED:
        return True
    if path in (
        {"openings.json", "bound-task.json", "reach-checkpoint.json", "training.json"}
        if robot == "G1"
        else {"private-layouts.json"}
    ):
        return True
    match = re.fullmatch(
        r"(?:case-(\d{3})\.log|attempt-(\d{3})\.json|case-(\d{3})/(.+))", path
    )
    if not match or int(next(x for x in match.groups()[:3] if x is not None)) >= count:
        return False
    name = match[4]
    if name is None or name in SPEC:
        return True
    if robot == "G1":
        return name in cohort.g1.FILES
    return (
        name in NANO
        or re.fullmatch(r"release-observations/[0-9]{6}\.npz", name) is not None
    )


def export(directory, target):
    with cohort.exclusive(directory / "operation.lock"):
        report = review.verify(directory)
        with NamedTemporaryFile(dir=target.parent) as temp:
            with ZipFile(temp.name, "w", ZIP_DEFLATED, compresslevel=6) as z:
                for p in sorted(directory.rglob("*")):
                    rel = p.relative_to(directory).as_posix()
                    if allowed(rel, report["robot"], report["cases"]) and p.is_file():
                        z.write(p, rel)
            # Do not replace a previously published archive.
            os.link(temp.name, target)
    return cohort.sha(target)


def restore(archive, state, identity, archive_digest):
    destination = cohort.directory_for(state, identity)
    if destination.exists():
        raise ValueError(
            "This registration already exists in the selected Studio project"
        )
    if cohort.sha(archive) != archive_digest:
        raise ValueError("The archive differs from the published evidence digest")
    state.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(dir=state) as temp:
        directory = Path(temp) / identity
        directory.mkdir()
        with ZipFile(archive) as z:
            infos = z.infolist()
            if (
                len(infos) > 12000
                or len({i.filename for i in infos}) != len(infos)
                or sum(i.file_size for i in infos) > MAX_TOTAL
                or any(
                    i.file_size > MAX_FILE or stat.S_ISLNK(i.external_attr >> 16)
                    for i in infos
                )
            ):
                raise ValueError("Unexpected archive size or entries")
            import json

            reg = json.loads(z.read("registration.json"))
            if cohort.digest(cohort.encode(reg)) != identity:
                raise ValueError(
                    "Archive registration differs from the published commitment"
                )
            robot = reg["robot"]
            count = {"G1": 20, "Nano": 50}[robot]
            for info in infos:
                if not allowed(info.filename, robot, count):
                    raise ValueError("Unexpected qualification archive entry")
                path = directory / info.filename
                path.parent.mkdir(parents=True, exist_ok=True)
                with z.open(info) as src, path.open("xb") as dst:
                    shutil.copyfileobj(src, dst, 128 * 1024)
        stored = cohort.read(directory / "verification.json")
        submitted_rows = cohort.read(directory / "qualification-rows.json")
        if cohort.digest(cohort.encode(submitted_rows)) != stored["rows_sha256"]:
            raise ValueError("Submitted case rows differ from the report")
        verified = review.verify(directory)
        if verified != stored:
            raise ValueError("Replayed measurements differ from the published verdict")
        directory.rename(destination)
        return verified


if __name__ == "__main__":
    import json

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["export", "restore"])
    parser.add_argument("--state", required=True, type=Path)
    parser.add_argument("--id", required=True)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--sha256")
    args = parser.parse_args()
    if args.action == "export":
        print(export(cohort.directory_for(args.state, args.id), args.archive))
    else:
        print(
            json.dumps(
                restore(args.archive, args.state, args.id, args.sha256), indent=2
            )
        )
