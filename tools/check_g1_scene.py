"""Bind a bounded G1 standing reach to a measured room and exercise it through native Studio."""

import argparse
import hashlib
import json
from pathlib import Path

from check_experiment import experiment_fixture
from studio_native import NativeStudio


def read(path):
    return json.loads(path.read_bytes())


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for key in ("studio", "remote", "session"):
        cli.add_argument("--" + key, type=Path, required=True)
    cli.add_argument(
        "--backend", choices=["headless", "offscreen"], default="offscreen"
    )
    cli.add_argument(
        "--provider", choices=["reference", "octos", "codex"], default="reference"
    )
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    survey = folder / "state/aisle/survey"
    task, _ = experiment_fixture(survey)
    app = NativeStudio(args, folder)
    p = "aisle_task.project_task."

    def show(key, part, timeout=40):
        return app.wait(
            lambda s: part in s["controls"].get(key, {}).get("text", ""),
            f"{key} showing {part}",
            timeout,
        )

    def field(key, value):
        app.scroll_to(key, p + "binding_inspector")
        app.click(key)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + str(value), "wait=1")

    def grab(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    with app.launch():
        app.click("aisle_task.g1")
        app.click("aisle_task.select_scene")
        show(p + "selection", "foot support")
        field(p + "g1_mass", "0.35")
        app.click(p + "activate")
        show(p + "result", "Impact must be")
        assert not (survey / "active-task-g1").exists()
        field(p + "g1_mass", "0.23")
        field(p + "g1_speed", "1.17")
        app.click(p + "activate")
        show(p + "result", "saved")
        grab("standing-binding")
        sha = (survey / "active-task-g1").read_text()
        frozen = survey / "tasks" / (sha + ".json")
        assert hashlib.sha256(frozen.read_bytes()).hexdigest() == sha
        bound = read(frozen)
        assert bound["standing"]["impact"] == dict(
            mass_kg=0.23, speed_m_s=1.17, lateral_m=0.0
        )
        app.click("aisle_task.select_scene")
        app.click("aisle_task." + args.provider)
        app.wait(
            lambda s: isinstance(s["state"]["controller"], dict)
            and s["state"]["controller"].get("bound_task_sha256") == sha
            and not s["state"]["busy"],
            "scene-bound G1 controller",
            timeout=300,
        )
        app.click("tabs.plugin_tab_aisle_run")
        app.click("aisle_run.start_run")
        app.wait(
            lambda s: not s["state"]["busy"]
            and (s["state"].get("result") or {}).get("bound_task_sha256") == sha,
            "measured G1 standing run",
            timeout=300,
        )
        app.click("tabs.plugin_tab_aisle_evidence")
        grab("standing-verdict")
    run = next(
        p.parent
        for p in (folder / "state/aisle").glob("*/result.json")
        if read(p).get("bound_task_sha256") == sha
    )
    r = read(run / "result.json")
    src = read(run / "source.json")
    bodies = r["scene_execution"]["bodies"]
    assert {b["instance"] for b in bodies if b["instance"]} == {
        i["id"] for i in task["scene"]["instances"]
    }
    assert (
        r["task_success"] and r["wrist_reach"]["passed"] and r["disturbance"]["passed"]
    )
    assert (
        r["both_feet_loaded_steps"] == r["physics_ticks"] == 2000
        and not r["scene_contacts"]
        and r["root_pose_writes"] == 0
    )
    assert (
        src["disturbance"]["mass_kg"] == 0.23
        and r["scene_execution"]["task_sha256"] == sha
    )
    camera = read(run / "recording.json")
    assert camera["cameras"] == ["operator"]
    assert len(list((run / "camera").glob("*.png"))) == len(camera["samples"])
    proof = dict(
        backend=args.backend,
        provider=args.provider,
        scene=r["scene_execution"]["scene"],
        task_sha256=sha,
        model_sha256=r["model_hash"],
        controller_sha256=r["controller_hash"],
        result_sha256=hashlib.sha256((run / "result.json").read_bytes()).hexdigest(),
        standing=bound["standing"],
        both_feet_loaded_steps=r["both_feet_loaded_steps"],
        wrist=r["wrist_reach"],
        impact=r["disturbance"],
        room_contacts=r["scene_contacts"],
        measured_bodies=bodies,
        positive_work_j=r["estimated_positive_joint_work_j"],
        max_excursion_rad=r["max_joint_excursion_rad"],
        operator_frames=len(camera["samples"]),
        root_pose_writes=r["root_pose_writes"],
    )
    with app.launch():
        app.click("aisle_task.g1")
        app.click("tabs.plugin_tab_aisle_run")
        app.click("aisle_run.experiment_mode")
        ep = "aisle_run.experiment_pane.exp_"
        app.click(ep + "count")
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=1", "wait=1")
        app.click(ep + "trace")
        app.click(ep + "history")
        app.click(ep + "start")
        show(ep + "feedback", "Batch started")
        show(ep + "rows", "completed", 300)
        batch = next((folder / "state/aisle/experiments").glob("*/batch.json"))
        row = read(batch)["repetitions"][0]
        trial = batch.parent / "trials" / row["run_id"]
        trace = [
            json.loads(raw)
            for raw in (trial / "robot-trace.jsonl").read_text().splitlines()
        ]
        assert len(trace) == 2000 and all(
            v["physics_step"] == i
            and len(v["joints"]) == len(v["positions_rad"]) == len(v["commanded"]) == 29
            and v["sim_time_ns"] == i * 5000000
            and v["both_feet_loaded"]
            and not v["collision_count"]
            for i, v in enumerate(trace, 1)
        )
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click("aisle_evidence.ev_mode")
        ep = "aisle_evidence.ev_catalog.ev_"
        show(ep + "clock", "0.10 s")
        show(ep + "trace", "29 joints")
        app.wait(
            lambda s: s["controls"][ep + "camera"].get("selected_label")
            == "Operator camera",
            "operator-only camera",
        )
        app.click(ep + "forward")
        show(ep + "clock", "0.20 s")
        show(ep + "trace", "step 40")
        grab("standing-recording")
        proof["batch"] = dict(
            run_id=row["run_id"],
            trace_steps=len(trace),
            operator_frames=len(read(trial / "recording.json")["samples"]),
            channels=row["channels"],
            replayed_physics_step=40,
            replayed_time_s=0.20,
            measured_verdict=row["outcome"],
        )
    with app.launch():
        app.click("aisle_task.g1")
        app.click("aisle_task.select_scene")
        show(p + "selection", "foot support")
        show(p + "g1_mass", "0.23")
        app.click("aisle_task.select_scene")
        app.click("aisle_task.nano")
        app.click("aisle_task.select_scene")
        show(p + "result", "Multiple graspable")
        assert (survey / "active-task-g1").read_text() == sha and not (
            survey / "active-task"
        ).exists()
    (folder / "verification.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(
        f"{args.backend}: G1 completed {r['physics_ticks']} steps; measured room bodies {len(bodies)}, final wrist error {r['wrist_reach']['final_error_m']:.4f} m"
    )


if __name__ == "__main__":
    main()
