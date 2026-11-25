"""Author named lighting, friction and pose ranges, then compare the previews with Nexus."""

import argparse
from contextlib import ExitStack
import hashlib
import json
import math
import os
import re
import signal
import subprocess
from pathlib import Path
from check_experiment import experiment_fixture
from studio_native import NativeStudio


def read(path):
    return json.loads(path.read_bytes())


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for name in ["studio", "session", "remote"]:
        cli.add_argument("--" + name, type=Path, required=True)
    cli.add_argument("--backend", choices=["headless", "offscreen"], default="headless")
    cli.add_argument("--robot", choices=["G1", "Nano"], default="G1")
    cli.add_argument("--worker", help="Registered experiment worker alias")
    cli.add_argument(
        "--incompatible-worker", help="Registered worker without the batch protocol"
    )
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    survey = folder / "state/aisle/survey"
    initial, _ = experiment_fixture(survey)
    revisions = survey / "scenes" / initial["scene"]["id"]
    old = (revisions / "1.json").read_bytes()
    app = NativeStudio(args, folder)
    editor = "aisle_scene.composer."
    run = "aisle_run.experiment_pane.exp_"
    inspector = editor + "inspector"
    form = run + "form"

    def click(key, panel=None):
        if panel:
            app.scroll_to(key, panel)
        app.click(key)

    def field(key, value, panel):
        click(key, panel)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + str(value), "wait=1")

    def choose(key, idx, panel):
        click(key, panel)
        for action in ["Home", *(["ArrowDown"] * idx), "ReturnKey"]:
            app.act("key", "w=0", "c=" + action, "wait=1")

    def show(key, value):
        return app.wait(
            lambda s: value in s["controls"].get(key, {}).get("text", ""),
            key + " " + value,
        )

    def shot(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    definitions = [
        ("Window light", 5, None, 0.25, 0.75),
        ("Table grip", 4, 0, 0.3, 0.8),
        ("Chair position", 0, 3, -1.65, -1.05),
        ("Chair angle", 3, 3, -18, 18),
    ]
    active_lease = None
    incompatible_rejected = False
    with ExitStack() as application:
        application.enter_context(app.launch())
        click("tabs.plugin_tab_aisle_scene")
        click("aisle_scene.composer_mode")
        click(editor + "open_scene")
        show(editor + "header", "5 objects")
        for name, prop, target, lo, hi in definitions:
            choose(editor + "rule_list", 0, inspector)
            choose(editor + "rule_property", prop, inspector)
            if target is not None:
                choose(editor + "rule_target", target, inspector)
            field(editor + "rule_name", name, inspector)
            field(editor + "rule_min", lo, inspector)
            if prop == 5:
                field(editor + "rule_max", 1.5, inspector)
                click(editor + "rule_apply", inspector)
                show(editor + "feedback", "from 0 to 1")
            field(editor + "rule_max", hi, inspector)
            click(editor + "rule_apply", inspector)
            show(editor + "feedback", "Save the scene revision")
        shot("scene-ranges")
        click(editor + "save")
        show(editor + "feedback", "Scene revision saved")
    scene = read(revisions / "2.json")
    assert (revisions / "1.json").read_bytes() == old
    assert [r["name"] for r in scene["rules"]] == [r[0] for r in definitions], scene[
        "rules"
    ]
    assert [r["instance"] for r in scene["rules"]] == [
        None,
        "object-oak",
        "chair-left",
        "chair-left",
    ]
    with app.launch():
        click("aisle_task." + args.robot.lower())
        click("aisle_task.select_scene")
        if args.robot == "Nano":
            choose(
                "aisle_task.project_task.pickup",
                1,
                "aisle_task.project_task.binding_inspector",
            )
        click("aisle_task.project_task.activate")
        show("aisle_task.project_task.result", "saved")
        task_hash = (
            survey / ("active-task-g1" if args.robot == "G1" else "active-task")
        ).read_text()
        frozen = survey / "tasks" / (task_hash + ".json")
        assert read(frozen)["scene"] == scene
        click("aisle_task.select_scene")
        click("aisle_task.reference")
        app.wait(
            lambda s: (s["state"].get("controller") or {}).get("bound_task_sha256")
            == task_hash
            and not s["state"]["busy"],
            "frozen G1 task",
        )
        click("tabs.plugin_tab_aisle_run")
        click("aisle_run.experiment_mode")
        if args.worker:
            aliases = [
                "local",
                *sorted(read(Path(os.environ["DOROBOT_AISLE_WORKERS"]))),
            ]
            if args.incompatible_worker:
                choose(run + "worker", aliases.index(args.incompatible_worker), form)
                show(run + "worker_info", "support frozen experiment batches")
                app.wait(
                    lambda s: s["controls"][run + "start"].get("enabled") is False,
                    "incompatible worker rejected",
                )
                incompatible_rejected = True
            choose(run + "worker", aliases.index(args.worker), form)
            show(run + "worker_info", "slots occupied")
            shot("worker-config")
            click(run + "trace", form)
            click(run + "history", form)
        field(run + "count", 2, form)
        click(run + "cameras", form)
        click(run + "preview")
        show(run + "preview_status", "Seed 42")
        assert "Window light" not in app.control(run + "preview_values")["text"]
        click(run + "preview")
        for idx, (name, *_) in enumerate(definitions):
            choose(run + "rule", idx, form)
            click(run + "rule_toggle", form)
            show(run + "rule_toggle", "Disable")
        click(run + "save")
        saved = next((folder / "state/aisle/experiment-configs").glob("*.json"))
        assert read(saved)["active_scene_rules"] == [r[0] for r in definitions]
        click(run + "preview")
        previews = []
        for idx in range(2):
            if idx:
                click(run + "preview_next")
            snap = app.wait(
                lambda s: f"Seed {42 + idx} ·"
                in s["controls"][run + "preview_status"]["text"]
                and "Window light" in s["controls"][run + "preview_values"]["text"],
                "sampled ranges in preview",
            )["controls"]
            values = snap[run + "preview_values"]["text"]
            assert all(r[0] in values for r in definitions), values
            previews.append(snap[run + "preview_status"]["text"].split(" · ")[-1])
        shot("range-preview")
        click(run + "start")
        show(run + "feedback", "Batch started")
        if args.worker:

            def active(_):
                nonlocal active_lease
                for manifest in (folder / "state/aisle/experiments").glob(
                    "*/batch.json"
                ):
                    for row in read(manifest)["repetitions"]:
                        if row.get("worker", {}).get("state") == "running":
                            active_lease = row["run_id"]
                            return True
                return False

            app.wait(active, "registered worker running")
            manifest = next((folder / "state/aisle/experiments").glob("*/batch.json"))
            processes = subprocess.check_output(
                ["ps", "-axo", "pid=,ppid=,command="], text=True
            )
            owned = []
            for line in processes.splitlines():
                pid, parent, cmd = line.strip().split(None, 2)
                if int(parent) == app.proc.pid and cmd.endswith(
                    "experiment_job.py run " + str(manifest.parent)
                ):
                    owned.append(int(pid))
            assert len(owned) == 1, "Expected the test Studio's own batch supervisor"
            os.kill(owned[0], signal.SIGTERM)
            application.close()
            application.enter_context(app.launch())
            click("aisle_task." + args.robot.lower())
            click("tabs.plugin_tab_aisle_run")
            click("aisle_run.experiment_mode")
            show(run + "worker_info", "slots occupied")
        terminal = re.compile(
            r"\bSeed \d+\s+(completed|failed|cancelled|skipped)\s+Trace"
        )
        states = app.wait(
            lambda s: len(terminal.findall(s["controls"][run + "rows"]["text"])) == 2,
            "two terminal " + args.robot + " simulations",
            720,
        )
        rows = states["controls"][run + "rows"]["text"]
        assert terminal.findall(rows) == ["completed", "completed"], rows
    with app.launch():
        click("aisle_task." + args.robot.lower())
        click("tabs.plugin_tab_aisle_run")
        click("aisle_run.experiment_mode")
        choose(run + "rule", 1, form)
        show(run + "rule_toggle", "Disable")
        show(run + "rule_info", "Surface friction")
        click(run + "preview")
        show(run + "preview_status", previews[0])
        show(run + "preview_values", "Chair position")
    manifest = next((folder / "state/aisle/experiments").glob("*/batch.json"))
    job = read(manifest)
    trials = []
    assert job["configuration"]["active_scene_rules"] == [r[0] for r in definitions]
    for idx, r in enumerate(job["repetitions"]):
        trial = manifest.parent / "trials" / r["run_id"]
        layout = read(trial / "sampled-layout.json")
        result = read(trial / "result.json")
        if args.worker:
            origin = read(trial / "worker-origin.json")
            assert (
                origin["node"] == args.worker
                and origin["job"] == r["worker"]["lease_id"] == r["run_id"]
            )
            assert (
                r["worker"]["id"]
                == job["worker"]["worker_id"]
                == read(saved)["worker_id"]
            )
            assert all(c["status"] == "complete" for c in r["channels"].values())
            assert (trial / "robot-trace.jsonl").is_file() and (
                trial / "agent-history.jsonl"
            ).is_file()
            assert not (trial / "recording.json").exists()
        values = layout["values"]
        measure = result["scene_execution"]
        assert (
            r["sample_sha256"].startswith(previews[idx])
            and hashlib.sha256(
                json.dumps(layout, sort_keys=True, separators=(",", ":")).encode()
            ).hexdigest()
            == r["sample_sha256"]
        )
        assert result["task_success"] and result["robot"] == args.robot
        robot_evidence = {}
        if args.robot == "G1":
            assert result["both_feet_loaded_steps"] == result["physics_ticks"] == 2000
        else:
            cube = result["cubes"]["red"]
            assert result["requested"] == ["red"] and cube["passed"]
            mode = read(trial / "controller.json")["table_perception"]
            assert mode == "oak-rgb-exposure-v2"
            observations = [
                json.loads(line)
                for line in (trial / "actions.jsonl").read_text().splitlines()
            ]
            dim = sum(row.get("exposure_mode") == "dim-oak" for row in observations)
            if r["seed"] == 43:
                assert dim > 0, (
                    "The low-light trial must use an onboard stereo observation"
                )
            robot_evidence.update(
                red_cube=cube, table_perception=mode, dim_oak_observations=dim
            )
        for name, _, _, lo, hi in definitions:
            assert (
                lo <= values[name]["value"] <= hi
                and values[name]["source"] == "scene_rule"
            )
        ambient = measure["ambient_rgb"]
        assert ambient == [values["Window light"]["value"]] * 3
        colliders = [b for b in measure["bodies"] if b["instance"] == "object-oak"]
        assert len(colliders) == 5 and all(
            b["friction"] == values["Table grip"]["value"] for b in colliders
        )
        chair = next(b for b in measure["bodies"] if b["instance"] == "chair-left")
        x = chair["measured_center_m"][0]
        qw, _, _, qz = chair["measured_quat_wxyz"]
        yaw = math.degrees(2 * math.atan2(qz, qw))
        assert (
            abs(x - values["Chair position"]["value"]) < 1e-5
            and abs(yaw - values["Chair angle"]["value"]) < 1e-4
        ), (x, yaw, values)
        trials.append(
            dict(
                seed=r["seed"],
                sample_sha256=r["sample_sha256"],
                physics_steps=result["physics_ticks"],
                task_success=result["task_success"],
                **robot_evidence,
                **(
                    {
                        "lease_id": origin["job"],
                        "archive_sha256": origin["sha256"],
                        "channels": r["channels"],
                    }
                    if args.worker
                    else {}
                ),
                values=values,
                nexus_ambient_rgb=ambient,
                table_collider_friction=colliders[0]["friction"],
                measured_chair_x_m=x,
                measured_chair_yaw_deg=yaw,
            )
        )
    assert (
        trials[0]["values"] != trials[1]["values"]
        and hashlib.sha256(frozen.read_bytes()).hexdigest() == task_hash
    )
    if args.worker:
        assert active_lease in [t["lease_id"] for t in trials]
    (folder / "verification.json").write_text(
        json.dumps(
            dict(
                backend=args.backend,
                robot=args.robot,
                task_sha256=task_hash,
                rules=scene["rules"],
                trials=trials,
                restored=True,
                **(
                    {
                        "worker": args.worker,
                        "worker_backend": job["worker"]["backend"],
                        "nexus": job["worker"]["nexus"],
                        "active_lease_before_restart": active_lease,
                        "same_lease_imported": active_lease
                        in [t["lease_id"] for t in trials],
                        "local_supervisor_restarted": True,
                        "incompatible_worker_rejected": incompatible_rejected,
                    }
                    if args.worker
                    else {}
                ),
            ),
            indent=2,
        )
        + "\n"
    )
    print(
        f"Verified four saved scene ranges across two {args.robot} physics runs",
        flush=True,
    )


if __name__ == "__main__":
    main()
