"""Seed a two-chair pickup experiment using metric assets for Studio's Run acceptance test."""

import copy
import json
from pathlib import Path
import tempfile

from check_bound_pickup import fixture, INSTRUCTION
from check_scene_editor import fixtures


def experiment_fixture(root):
    scene, assets = fixture(root)
    with tempfile.TemporaryDirectory() as temp:
        fixtures(Path(temp))
        chair = json.loads((Path(temp) / "assets/fixture-chair/1.json").read_bytes())
    chair["physics"]["appearance"] = {
        "value": "neutral",
        "provenance": {"source": "operator"},
    }
    chair["physics"]["graspable"] = {
        "value": False,
        "provenance": {"source": "operator"},
    }
    pin = dict(id=chair["id"], revision=1)
    dst = root / "assets" / chair["id"] / "1.json"
    dst.parent.mkdir()
    dst.write_text(json.dumps(chair))
    assets.append(chair)
    for name, x in [("left", -1.2), ("right", 1.2)]:
        scene["instances"].append(
            dict(
                id="chair-" + name,
                name=name.title() + " chair",
                asset=pin,
                position_m=[x, -1.7, 0.0],
                yaw_deg=0,
                support=None,
            )
        )
    (root / "scenes" / scene["id"] / "1.json").write_text(json.dumps(scene))
    task = dict(
        schema="aisle-studio-nano-pickup-v1",
        robot="Nano",
        instruction=INSTRUCTION,
        scene=scene,
        assets=assets,
        pickup="object-red",
        placement=dict(instance="object-oak", surface="Landing area"),
    )
    config = dict(
        name="Chair spacing",
        repeat_count=4,
        seed_mode="sequence",
        base_seed=42,
        recording=dict(robot_trace=False, agent_history=False),
        overrides={
            "2": {"robot_trace": True},
            "3": {"agent_history": True},
            "4": {"robot_trace": True, "agent_history": True},
        },
        parameters=[
            dict(
                name="Chair spacing",
                type="number",
                unit="m",
                default=2.4,
                range=[1.9, 2.8],
                binding=dict(
                    kind="spacing", axis="x", instances=["chair-left", "chair-right"]
                ),
            )
        ],
        record_cameras=False,
    )
    return task, config


def verify(folder, job):
    import hashlib, math
    from experiment import sample, sha, encode

    batch = folder / "state/aisle/experiments" / job["batch_id"]
    task = json.loads((batch / "controller/bound-task.json").read_bytes())
    rows = job["repetitions"]
    assert [r["status"] for r in rows] == ["completed"] * 4 + ["cancelled"], [
        (r["status"], r.get("reason")) for r in rows
    ]
    assert len({r["run_id"] for r in rows}) == 5
    receipt = []
    for i, r in enumerate(rows[:4]):
        trial = batch / "trials" / r["run_id"]
        m = json.loads((trial / "manifest.json").read_bytes())
        assert m == r
        layout = json.loads((trial / "sampled-layout.json").read_bytes())
        assert (
            r["seed"] == 42 + i
            and layout == sample(task, job["configuration"], 42 + i)
            and sha(encode(layout)) == r["sample_sha256"]
        )
        d = json.loads((trial / "result.json").read_bytes())
        assert (
            hashlib.sha256((trial / "result.json").read_bytes()).hexdigest()
            == r["outcome"]["result_sha256"]
            and d["task_success"]
        )
        bodies = {b["instance"]: b for b in d["scene_execution"]["bodies"]}
        spacing = (
            bodies["chair-right"]["measured_center_m"][0]
            - bodies["chair-left"]["measured_center_m"][0]
        )
        assert abs(spacing - layout["values"]["Chair spacing"]["value"]) < 1e-5
        trace = trial / "robot-trace.jsonl"
        history = trial / "agent-history.jsonl"
        assert trace.exists() == (i in (1, 3)) and history.exists() == (i in (2, 3))
        if trace.exists():
            count = 0
            with trace.open() as f:
                for count, raw in enumerate(f, 1):
                    point = json.loads(raw)
                    assert (
                        point["physics_step"] == count
                        and point["run_id"] == r["run_id"]
                        and point["sim_time_ns"] == count * 10000000
                    )
                    assert (
                        len(point["joints"])
                        == len(point["positions_rad"])
                        == len(point["commanded"])
                        == 17
                        and point["wheel_supported"]
                    )
            assert count == d["physics_ticks"]
        if history.exists():
            events = [json.loads(s) for s in history.read_text().splitlines()]
            assert all(
                e["run_id"] == r["run_id"]
                and e["controller_sha256"] == r["controller_sha256"]
                for e in events
            )
            plan = json.loads((batch / "controller/controller.json").read_bytes())
            kinds = {e["event"] for e in events}
            if plan["provider"] == "reference":
                assert "reference_controller" in kinds
            else:
                assert {"prompt", "response"} <= kinds and any(
                    e["event"] == "prompt" and e["data"]["text"] for e in events
                )
            assert "controller_reused" in kinds
        receipt.append(
            dict(
                run_id=r["run_id"],
                seed=r["seed"],
                status=r["status"],
                physics_ticks=d["physics_ticks"],
                sample_sha256=r["sample_sha256"],
                measured_chair_spacing_m=spacing,
                channels=r["channels"],
                outcome=r["outcome"],
            )
        )
    return dict(
        batch_id=job["batch_id"],
        configuration=job["configuration"],
        completed=receipt,
        cancelled=rows[4],
    )


def main():
    import argparse, sys, time
    from studio_native import NativeStudio

    sys.path.insert(0, str(Path(__file__).parent / "aisle_workbench"))
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--studio", type=Path, required=True)
    cli.add_argument("--remote", type=Path, required=True)
    cli.add_argument("--session", type=Path, required=True)
    cli.add_argument(
        "--backend", default="offscreen", choices=["offscreen", "headless"]
    )
    cli.add_argument(
        "--provider", default="reference", choices=["reference", "codex", "octos"]
    )
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    task, config = experiment_fixture(folder / "state/aisle/survey")
    app = NativeStudio(args, folder)
    prefix = "aisle_run.experiment_pane.exp_"

    def show(key, text):
        return app.wait(
            lambda s: text in s["controls"].get(key, {}).get("text", ""),
            f"{key} to contain {text}",
        )

    form_controls = set(
        "name count seed_mode seed scope trace history cameras inherit features feature type units binding a b axis default low high choices feature_save feature_delete".split()
    )

    def target(key):
        if key in form_controls:
            app.scroll_to(prefix + key, prefix + "form")
        app.click(prefix + key)

    def textfield(key, value):
        target(key)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + str(value), "wait=1")

    def choose(full, delta):
        app.click(full)
        x, y, w, h = app.control(full)["rect"]
        app.act("click", "w=0", f"x={x+25}", f"y={y+h/2+delta*30}", "wait=1")

    def list_(key, index, label):
        control = prefix + key
        app.scroll_to(control, prefix + "form")
        app.click(control)
        for keycode in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + keycode, "wait=1")
        app.wait(
            lambda s: s["controls"][control]["selected_label"] == label,
            f"{control} selection {label}",
        )

    def batch_page():
        app.click("tabs.plugin_tab_aisle_run")
        app.click("aisle_run.experiment_mode")

    def grab(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    with app.launch():
        app.click("aisle_task.select_scene")
        bind = "aisle_task.project_task."
        show(bind + "result", "Multiple graspable objects")
        choose(bind + "pickup", 1)
        show(bind + "selection", "Red cube")
        app.click(bind + "activate")
        show(bind + "result", "saved")
        app.click("aisle_task.select_scene")
        app.click("aisle_task." + args.provider)
        app.wait(
            lambda s: isinstance(s["state"]["controller"], dict)
            and s["state"]["controller"].get("bound_task_sha256")
            and not s["state"]["busy"],
            "prepare experiment controller",
            timeout=300,
        )
        batch_page()
        textfield("name", "Chair spacing")
        textfield("count", "5")
        target("cameras")
        show(prefix + "cameras", "off")
        list_("scope", 2, "Repetition 2")
        target("trace")
        list_("scope", 3, "Repetition 3")
        target("history")
        list_("scope", 4, "Repetition 4")
        target("trace")
        target("history")
        textfield("feature", "Chair spacing")
        textfield("units", "m")
        textfield("default", "2.4")
        textfield("low", "1.9")
        textfield("high", "2.8")
        list_("a", 3, "Left chair")
        list_("b", 4, "Right chair")
        target("feature_save")
        show(prefix + "feedback", "Updated runtime features")
        target("save")
        show(prefix + "feedback", "Configuration saved")
        target("start")
        show(prefix + "feedback", "Batch started")
        path = next((folder / "state/aisle/experiments").glob("*/batch.json"))
        job = json.loads(path.read_bytes())
        assert job["configuration"]["overrides"] == {
            "2": {"robot_trace": True},
            "3": {"agent_history": True},
            "4": {"robot_trace": True, "agent_history": True},
        }, job["configuration"]
        feature = job["configuration"]["parameters"][0]
        assert feature["binding"]["instances"] == ["chair-left", "chair-right"], feature
        for _ in range(4):
            target("next")
        show(prefix + "detail", job["repetitions"][-1]["run_id"])
        target("cancel")
        show(prefix + "feedback", "Cancellation requested")
        grab("batch-started")
    with app.launch():
        batch_page()
        show(prefix + "summary", "5 repetitions")
        state = app.wait(
            lambda s: "cancelled"
            in s["controls"].get(prefix + "rows", {}).get("text", ""),
            "four Nexus repetitions and retained cancellation",
            timeout=1800,
        )
        job = json.loads(path.read_bytes())
        receipt = verify(folder, job)
        target("next")
        show(prefix + "detail", job["repetitions"][1]["run_id"])
        grab("batch-results")
    with app.launch():
        batch_page()
        show(prefix + "rows", "cancelled")
        show(prefix + "summary", "5 repetitions")
        receipt["backend"] = args.backend
        receipt["provider"] = args.provider
        (folder / "verification.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(
        f"{args.backend}: four real trials, four recording combinations, measured chair spacing, reproducible inputs and one durable cancellation"
    )


if __name__ == "__main__":
    main()
