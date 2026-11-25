"""Version the operator's notes and derive reruns from a frozen experiment trial."""

import argparse
from copy import deepcopy
import fcntl
import json
from pathlib import Path
from uuid import uuid4
from tempfile import TemporaryDirectory

from experiment import encode, sample, sha
from experiment_job import (
    TERMINAL,
    create,
    freeze_controller,
    read,
    trial_path,
    uid,
    write,
)
from experiment_recording import now
from worker import digest


def source(folder, run_id):
    uid(run_id)
    job = read(folder / "batch.json")
    row = next((r for r in job["repetitions"] if r["run_id"] == run_id), None)
    if not row or row["status"] not in TERMINAL:
        raise ValueError("Select a finished trial")
    trial = trial_path(folder, row)
    if read(trial / "manifest.json") != row:
        raise ValueError("The batch index and trial manifest disagree")
    return job, row, trial


def annotations(folder, run_id, data):
    _, row, _ = source(folder, run_id)
    revision = data["revision"]
    fields = {k: data[k] for k in ["name", "notes", "tags", "fields"]}
    if not isinstance(fields["name"], str) or not fields["name"].strip():
        raise ValueError("Enter a run name")
    if not isinstance(fields["notes"], str) or not isinstance(fields["fields"], dict):
        raise ValueError("Use text notes and a JSON object for metadata")
    if not isinstance(fields["tags"], list) or not all(
        isinstance(s, str) for s in fields["tags"]
    ):
        raise ValueError("Enter text tags")
    folder = folder / "annotations" / row["run_id"]
    folder.mkdir(parents=True, exist_ok=True)
    with (folder / "edit.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        revisions = sorted(folder.glob("[0-9]*.json"))
        if revision != len(revisions):
            raise ValueError("Notes changed in another window. Refresh before editing")
        rev = revision + 1
        data = dict(**fields, revision=rev, run_id=run_id, updated=now())
        write(folder / f"{rev:08}.json", data)
        return data


def baseline(folder, run_id):
    job, row, trial = source(folder, run_id)
    cfg = deepcopy(job["configuration"])
    cfg.update(
        repeat_count=1,
        seed_mode="explicit",
        seeds=[row["seed"]],
        recording=row["recording"],
        overrides={},
    )
    inputs = folder / "controller"
    if (
        digest(inputs / "controller.json") != row["controller_sha256"]
        or digest(inputs / "bound-task.json") != row["task_sha256"]
    ):
        raise ValueError("Frozen task or controller checksum changed")
    task = read(inputs / "bound-task.json")
    original = sample(task, cfg, row["seed"])
    if sha(encode(original)) != row.get("sample_sha256") or original != read(
        trial / "sampled-layout.json"
    ):
        raise ValueError("Frozen inputs do not reproduce the recorded layout")
    if (
        row.get("outcome")
        and digest(trial / "result.json") != row["outcome"]["result_sha256"]
    ):
        raise ValueError("The parent's measured result checksum changed")
    parent = dict(
        batch_id=job["batch_id"],
        run_id=run_id,
        sample_sha256=row["sample_sha256"],
        task_sha256=row["task_sha256"],
        controller_sha256=row["controller_sha256"],
        measured_result_sha256=row.get("outcome", {}).get("result_sha256"),
    )
    return cfg, parent, task


def save_variant(folder, run_id, data):
    from authored_scene import load

    cfg, parent, task = baseline(folder, run_id)
    name = data["name"].strip()
    if not name:
        raise ValueError("Name the derived configuration")
    cfg["name"] = name
    cfg["pose_edits"] = deepcopy(cfg.get("pose_edits", {}))
    for key, axes in data.get("pose_edits", {}).items():
        cfg["pose_edits"].setdefault(key, {}).update(axes)
    if "parameters" in data:
        if not isinstance(data["parameters"], list):
            raise ValueError("Runtime features must be a list")
        cfg["parameters"] = data["parameters"]
    cfg["seeds"] = data.get("seeds", [data.get("seed", cfg["seeds"][0])])
    cfg["repeat_count"] = len(cfg["seeds"])
    if "recording" in data:
        cfg["recording"].update(data["recording"])
    if "record_cameras" in data:
        if type(data["record_cameras"]) is not bool:
            raise ValueError("Choose whether to record cameras")
        cfg["record_cameras"] = data["record_cameras"]
    from experiment import recordings, seeds

    original = folder / "controller/controller.json"
    source_path = (
        Path(data["prepared_controller"]) if "prepared_controller" in data else original
    )
    checksum = digest(source_path)
    if checksum != data.get("prepared_sha256", parent["controller_sha256"]):
        raise ValueError("Selected controller changed while preparing the variant")
    plan = read(source_path)
    bound = load(source_path.with_name("bound-task.json"), plan["bound_task_sha256"])
    if (
        plan["robot"] != bound.document["robot"]
        or plan.get("color_order") != bound.color
    ):
        raise ValueError("The selected controller and task have different bindings")
    cfg["scene"] = {
        "id": bound.document["scene"]["id"],
        "revision": bound.document["scene"]["revision"],
    }
    cfg["controller_sha256"] = checksum
    sample_hashes = []
    for index, seed in enumerate(seeds(cfg), 1):
        recordings(cfg, index)
        realized = sample(bound.document, cfg, seed)
        load(source_path.with_name("bound-task.json"), bound.sha256, realized)
        sample_hashes.append(sha(encode(realized)))
    variant = dict(
        id=str(uuid4()),
        name=name,
        created=now(),
        parent=parent,
        configuration=cfg,
        controller_sha256=checksum,
        task_sha256=bound.sha256,
        sample_sha256=sample_hashes[0],
        samples_sha256=sample_hashes,
    )
    target = folder / "variants" / run_id
    target.mkdir(parents=True, exist_ok=True)
    if checksum != parent["controller_sha256"]:
        with TemporaryDirectory(dir=target) as tmp:
            dest = Path(tmp) / "controller"
            dest.mkdir()
            freeze_controller(source_path, dest, plan, checksum, bound.sha256)
            dest.rename(target / variant["id"])
    write(target / (variant["id"] + ".json"), variant)
    return variant


def rerun(folder, run_id, variant_id=None):
    cfg, parent, task = baseline(folder, run_id)
    controller = folder / "controller/controller.json"
    checksum = parent["controller_sha256"]
    if variant_id:
        v = read(folder / "variants" / run_id / (uid(variant_id) + ".json"))
        if v["parent"] != parent or v["id"] != variant_id:
            raise ValueError("Variant input revisions differ from the selected parent")
        cfg = v["configuration"]
        checksum = v.get("controller_sha256", checksum)
        if checksum != parent["controller_sha256"]:
            controller = folder / "variants" / run_id / variant_id / "controller.json"
            task = read(controller.with_name("bound-task.json"))
        if digest(controller) != checksum or digest(
            controller.with_name("bound-task.json")
        ) != v.get("task_sha256", parent["task_sha256"]):
            raise ValueError("The variant's frozen controller or task has changed")
        if [sha(encode(sample(task, cfg, seed))) for seed in cfg["seeds"]] != v.get(
            "samples_sha256", [v["sample_sha256"]]
        ):
            raise ValueError("Variant layout checksum changed")
    else:
        cfg["name"] += " · original rerun"
    parent["variant_id"] = variant_id
    return create(
        folder.parent,
        cfg,
        controller,
        checksum,
        parent=parent,
    )


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("operation", choices=["annotate", "variant", "original", "edited"])
    cli.add_argument("batch", type=Path)
    cli.add_argument("run_id")
    cli.add_argument("input", nargs="?")
    a = cli.parse_args()
    if a.operation in ("annotate", "variant"):
        data = json.loads(a.input)
        print(
            json.dumps(
                (annotations if a.operation == "annotate" else save_variant)(
                    a.batch, a.run_id, data
                )
            )
        )
    else:
        if a.operation == "edited" and not a.input:
            raise ValueError("Choose a saved variant")
        print(rerun(a.batch, a.run_id, a.input if a.operation == "edited" else None))


if __name__ == "__main__":
    main()
