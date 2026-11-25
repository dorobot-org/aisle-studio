"""Drive recorded RGB, notes, a chair-position variant and its measured rerun through Studio."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from studio_native import NativeStudio, ROOT


def read(path):
    return json.loads(path.read_bytes())


def table(path):
    files = sorted(x for x in path.rglob("*") if x.is_file())
    return {
        str(x.relative_to(path)): hashlib.sha256(x.read_bytes()).hexdigest()
        for x in files
    }


def chair(trial):
    return next(
        b["measured_center_m"]
        for b in read(trial / "result.json")["scene_execution"]["bodies"]
        if b["instance"] == "chair-left"
    )


def setup(args, folder):
    source = args.source_batch.resolve()
    j = read(source / "batch.json")
    r = next(r for r in j["repetitions"] if r["status"] == "completed")
    store = folder / "state/aisle/experiments"
    store.mkdir(parents=True)
    if (source / "trials" / r["run_id"] / "recording.json").is_file() and len(
        j["repetitions"]
    ) == 1:
        batch = store / source.name
        shutil.copytree(
            source, batch, ignore=shutil.ignore_patterns("annotations", "variants")
        )
    else:
        cfg = j["configuration"] | dict(
            name="Living room replay",
            repeat_count=1,
            seed_mode="explicit",
            seeds=[r["seed"]],
            recording=dict(robot_trace=True, agent_history=True),
            overrides={},
            record_cameras=True,
        )
        path = folder / "baseline.json"
        path.write_text(json.dumps(cfg))
        cmd = [
            os.getenv("DOROBOT_AISLE_PYTHON", sys.executable),
            str(ROOT / "tools/aisle_workbench/experiment_job.py"),
        ]
        batch = Path(
            subprocess.check_output(
                cmd
                + [
                    "create",
                    str(store),
                    str(path),
                    str(source / "controller/controller.json"),
                ],
                text=True,
            ).strip()
        )
        subprocess.run(cmd + ["run", str(batch)], check=True)
    row = read(batch / "batch.json")["repetitions"][0]
    assert row["status"] == "completed", row
    return batch, row


def check_channels(args, folder):
    source = args.source_batch.resolve()
    path = folder / "state/aisle/experiments"
    path.mkdir(parents=True)
    (path / source.name).symlink_to(source, target_is_directory=True)
    job = read(source / "batch.json")
    assert not job["configuration"]["record_cameras"]
    rows = [r for r in job["repetitions"] if r["status"] == "completed"]
    assert len(rows) == 4
    app = NativeStudio(args, folder)
    prefix = "aisle_evidence.ev_catalog.ev_"
    receipt = []
    with app.launch():
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click("aisle_evidence.ev_mode")
        for r in rows:
            app.click(prefix + "search")
            app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
            app.act("text", "w=0", "t=" + r["run_id"], "wait=1")
            app.click(prefix + "filter")
            state = app.wait(
                lambda s: r["run_id"] in s["controls"][prefix + "verdict"]["text"],
                "selected trial",
            )["controls"]
            clock = state[prefix + "clock"]["text"]
            channels = state[prefix + "channels"]["text"]
            history = state[prefix + "event_detail"]["text"]
            assert "RGB · 0 frames" in channels, channels
            for k, title in [
                ("robot_trace", "Robot trace"),
                ("agent_history", "Agent history"),
            ]:
                assert (
                    title + " · " + ("recorded" if r["recording"][k] else "off")
                    in channels
                )
            if r["recording"]["robot_trace"]:
                assert "trace sample" in clock, clock
                app.click(prefix + "forward")
                app.wait(
                    lambda s: "step 21 " in s["controls"][prefix + "trace"]["text"],
                    "second trace sample",
                )
            else:
                assert "No camera recording" in clock, clock
            assert ("No agent events" not in history) == r["recording"]["agent_history"]
            receipt.append(
                dict(
                    run_id=r["run_id"],
                    recording=r["recording"],
                    channels=channels,
                    clock=clock,
                )
            )
    (folder / "channel-verification.json").write_text(
        json.dumps(receipt, indent=2) + "\n"
    )
    print(
        "Verified Evidence for all four trace/history combinations with the camera dataset disabled"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--studio", type=Path, required=True)
    parser.add_argument("--remote", type=Path, required=True)
    parser.add_argument("--source-batch", type=Path, required=True)
    parser.add_argument("--session", type=Path, required=True)
    parser.add_argument(
        "--backend", default="offscreen", choices=["offscreen", "headless"]
    )
    parser.add_argument(
        "--channels-only",
        action="store_true",
        help="Check Evidence display against the four-repeat recording test batch",
    )
    args = parser.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    if args.channels_only:
        return check_channels(args, folder)
    batch, row = setup(args, folder)
    trial = batch / "trials" / row["run_id"]
    frozen = table(trial)
    before = chair(trial)
    assert abs(before[1] + 1.7) < 1e-5
    cam = read(trial / "recording.json")
    samples = cam["samples"]
    assert len(samples) > 10
    app = NativeStudio(args, folder)
    p = "aisle_evidence.ev_catalog.ev_"
    inspector = set(
        "parent original name notes tags fields save_notes revisions variant_name object axis position seed save_variant variants edited events".split()
    )

    def enter():
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click("aisle_evidence.ev_mode")

    def target(k):
        if k in inspector:
            app.scroll_to(p + k, p + "inspector")
        app.click(p + k)

    def show(k, value, timeout=35):
        return app.wait(
            lambda s: value in s["controls"].get(p + k, {}).get("text", ""),
            f"{k} containing {value}",
            timeout,
        )

    def field(k, text):
        target(k)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + text, "wait=1")

    def select(k, index, label):
        target(k)
        for key in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + key, "wait=1")
        app.wait(
            lambda s: s["controls"][p + k]["selected_label"] == label,
            f"{k} to select {label}",
        )

    def grab(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    with app.launch():
        enter()
        show("clock", "frame 1 /")
        show("trace", "17 joints")
        show("channels", "Agent history · recorded")
        target("forward")
        show("clock", "frame 2 /")
        select("camera", 1, "Left wrist")
        target("play")
        show("play", "Pause")
        show("clock", "frame 3 /")
        target("play")
        show("play", "Play")
        x, y, w, h = app.control(p + "scrub")["rect"]
        for kind, fraction in [
            ("down", 0.06),
            ("move", 0.10),
            ("move", 0.25),
            ("move", 0.41),
            ("move", 0.61),
            ("up", 0.61),
        ]:
            app.act(
                "mouse", "k=" + kind, "w=0", f"x={x+w*fraction}", f"y={y+h/2}", "wait=1"
            )
        app.wait(
            lambda s: int(
                s["controls"][p + "clock"]["text"].split("frame ")[1].split(" /")[0]
            )
            > 5,
            "recorded timeline scrub",
        )
        visual = app.control(p + "clock")["text"]
        assert int(visual.split("frame ")[1].split(" /")[0]) > 5, visual
        phases = read(trial / "phases.json")
        n = next(i for i, s in enumerate(phases) if s["name"] == "place")
        phase = phases[n]
        select("phase", n, f"place · {phase['tick']*10} ms")
        expected = next(i for i, s in enumerate(samples) if s["step"] >= phase["tick"])
        show("clock", f"frame {expected+1} /")
        show("trace", f"step {samples[expected]['step']} ")
        grab("recorded-wrist")
        field("name", "Verified living-room pickup")
        field("notes", "Watch left wrist at placement")
        field("tags", "living-room, review")
        field("fields", '{"operator_check":"tabletop"}')
        target("save_notes")
        show("feedback", "Notes saved")
        field("search", "review")
        target("filter")
        show("verdict", row["run_id"])
        field("variant_name", "Chair farther from table")
        select("object", 3, "Left chair")
        select("axis", 1, "Y")
        field("position", "-2.0")
        target("save_variant")
        show("feedback", "Variant saved")
        show("diff", "Left chair y")
        grab("variant-changes")
    with app.launch():
        enter()
        show("revision_detail", "Watch left wrist")
        show("diff", "Left chair y")
        target("edited")
        show("feedback", "Derived run started")
        show("lineage", row["run_id"][:8])
        show("verdict", "completed", 1800)
        show("lineage", "Left chair")
        grab("parent-comparison")
        target("parent")
        show("verdict", row["run_id"])
        show("revision_detail", "Watch left wrist")
        target("original")
        show("feedback", "Derived run started")
        show("verdict", "completed", 1800)
    original = next(
        read(q)
        for q in batch.parent.glob("*/batch.json")
        if q.parent != batch
        and read(q).get("parent", {}).get("run_id") == row["run_id"]
        and not read(q)["parent"]["variant_id"]
    )
    same = original["repetitions"][0]
    assert (
        same["status"] == "completed" and same["sample_sha256"] == row["sample_sha256"]
    )
    same_dir = batch.parent / original["batch_id"] / "trials" / same["run_id"]
    assert read(same_dir / "sampled-layout.json") == read(trial / "sampled-layout.json")
    assert max(abs(a - b) for a, b in zip(chair(same_dir), before)) < 1e-5
    j = next(
        read(q)
        for q in batch.parent.glob("*/batch.json")
        if q.parent != batch
        and read(q).get("parent", {}).get("run_id") == row["run_id"]
        and read(q)["parent"]["variant_id"]
    )
    later = j["repetitions"][0]
    assert (
        later["run_id"] != row["run_id"]
        and later["parent_run_id"] == row["run_id"]
        and later["status"] == "completed"
    )
    child = batch.parent / j["batch_id"] / "trials" / later["run_id"]
    after = chair(child)
    assert abs(after[1] + 2.0) < 1e-5 and abs(after[1] - before[1]) > 0.29
    assert table(trial) == frozen
    metadata = read(next((batch / "annotations" / row["run_id"]).glob("*.json")))
    assert metadata["fields"] == {"operator_check": "tabletop"} and metadata[
        "tags"
    ] == ["living-room", "review"]
    assert (
        row["sample_sha256"] != later["sample_sha256"]
        and row["controller_sha256"] == later["controller_sha256"]
    )
    proof = dict(
        backend=args.backend,
        parent=row["run_id"],
        child=later["run_id"],
        original_rerun_id=same["run_id"],
        original_rerun_outcome=same["outcome"],
        original_rerun_sample_sha256=same["sample_sha256"],
        parent_verdict=row["outcome"],
        child_verdict=later["outcome"],
        original_sample_sha256=row["sample_sha256"],
        variant_sample_sha256=later["sample_sha256"],
        variant_id=later["variant_id"],
        frozen_artifact_count=len(frozen),
        frozen_tree_sha256=hashlib.sha256(
            json.dumps(frozen, sort_keys=True).encode()
        ).hexdigest(),
        recorded_frames_per_camera=len(samples),
        named_cameras=cam["cameras"],
        replayed_placement_step=samples[expected]["step"],
        measured_chair_original_m=before,
        measured_chair_variant_m=after,
        notes=metadata,
    )
    (folder / "verification.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(
        f"{args.backend}: scrubbed a {len(samples)}-frame recording, preserved {len(frozen)} artifacts, remeasured chair from {before[1]:.3f} to {after[1]:.3f} m"
    )


if __name__ == "__main__":
    main()
