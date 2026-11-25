"""Choose conflicting package sources in Task, exercise tamper rejection, and run through the SPEC bridge."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil

from check_experiment import experiment_fixture
from studio_native import NativeStudio, ROOT


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_bytes())


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for name in ("studio", "remote", "session"):
        cli.add_argument("--" + name, required=True, type=Path)
    cli.add_argument("--backend", choices=["offscreen", "headless"], default="headless")
    cli.add_argument("--robot", choices=["G1", "Nano"], required=True)
    args = cli.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    state = folder / "state/aisle"
    survey = state / "survey"
    experiment_fixture(survey)
    sample = ROOT / "skills/aisle-nexus-task"
    variant = folder / "reviewed-skill"
    invalid = folder / "privileged-skill"
    shutil.copytree(sample, variant)
    shutil.copytree(sample, invalid)
    main_file = variant / "main"
    # This alternate, version-identical package asks the deployment endpoint for a
    # privileged observation. Only proceed to the actual action if the node rejects it.
    original = main_file.read_bytes().replace(
        b"        reply = invoke(sys.argv[1], json.load(sys.stdin))",
        b"        args = json.load(sys.stdin)\n"
        b'        denied = invoke(sys.argv[1], args | {"observation_inputs": [*args["observation_inputs"], "simulator_object_poses"]})\n'
        b'        if denied["ok"] or denied["code"] != "OBSERVATION_DENIED":\n'
        b'            raise ValueError("Privileged observation was accepted")\n'
        b"        reply = invoke(sys.argv[1], args)",
    )
    assert original != main_file.read_bytes()
    main_file.write_bytes(original)
    manifest = read(invalid / "manifest.json")
    manifest["aisle"]["profiles"][args.robot]["sensors"].append(
        "simulator_object_poses"
    )
    (invalid / "manifest.json").write_text(json.dumps(manifest))
    registry = folder / "sources.json"
    registry.write_text(
        json.dumps({"Reviewed build": str(variant), "Privileged fixture": str(invalid)})
    )
    os.environ["DOROBOT_SPEC_SOURCES"] = str(registry)
    app = NativeStudio(args, folder)

    def choose(control, index):
        app.click(control)
        for key in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + key, "wait=1")

    def text(control, part):
        return app.wait(
            lambda s: part in s["controls"].get(control, {}).get("text", ""),
            control + " to show " + part,
        )

    def library():
        app.click("aisle_task.skill_mode")
        text("aisle_task.skill_status", "observation contract")

    with app.launch():
        app.click("aisle_task." + args.robot.lower())
        library()
        assert (
            app.control("aisle_task.skill_sources")["selected_label"]
            == "Choose installed source"
        )
        choose("aisle_task.skill_sources", 1)
        first = text("aisle_task.skill_contract", "Studio ·")["controls"][
            "aisle_task.skill_contract"
        ]["text"]
        choose("aisle_task.skill_sources", 2)
        second = text("aisle_task.skill_contract", "Reviewed build ·")["controls"][
            "aisle_task.skill_contract"
        ]["text"]
        assert first.split("SHA-256 ")[-1] != second.split("SHA-256 ")[-1]
        app.click("aisle_task.skill_use")
        selected = state / "spec-skills" / ("selected-" + args.robot + ".json")
        app.wait(
            lambda s: selected.exists()
            and read(selected).get("source") == "Reviewed build",
            "persist package selection",
        )
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / "package-selection.png"))
    with app.launch():
        if args.robot == "G1":
            app.click("aisle_task.g1")
        library()
        app.wait(
            lambda s: "Reviewed build"
            in (s["controls"]["aisle_task.skill_sources"].get("selected_label") or ""),
            "restore chosen source",
        )
        app.click("aisle_task.skill_mode")
        app.click("aisle_task.select_scene")
        if args.robot == "Nano":
            choose("aisle_task.project_task.pickup", 1)
            text("aisle_task.project_task.selection", "Red cube")
        app.click("aisle_task.project_task.activate")
        text("aisle_task.project_task.result", "saved")
        app.click("aisle_task.select_scene")
        app.click("aisle_task.reference")
        text("aisle_task.skill_applied", "Reviewed build")
        (prep,) = list(state.glob("*-prepare/controller.json"))
        lock = read(prep)["spec_skill"]
        assert lock["package"] == read(selected) and lock["package"][
            "main_sha256"
        ] == sha(main_file)
        app.click("tabs.plugin_tab_aisle_run")
        main_file.write_bytes(original + b"\n# revision changed after preparation\n")
        app.click("aisle_run.start_run")

        def rejected(_):
            return any(
                "installed skill differs" in read(p).get("message", "")
                for p in state.glob("*-run/error.json")
            )

        app.wait(rejected, "reject modified entry point")
        (error_file,) = state.glob("*-run/error.json")
        assert (
            not (error_file.parent / "spec-graph.log").exists()
            and not (error_file.parent / "result.json").exists()
        )
        app.wait(lambda s: not s["state"]["busy"], "finish rejected trial")
        main_file.write_bytes(original)
        stopped_groups = 0
        if args.robot == "Nano":
            app.click("aisle_run.start_run")
            app.wait(
                lambda s: any(state.glob("*-run/spec-denials.jsonl"))
                and any(state.glob("*-run/progress.json")),
                "active SPEC pickup before stopping",
                90,
            )
            (journal,) = state.glob("*-run/spec-denials.jsonl")
            stopped = journal.parent
            groups = [
                read(stopped / ("spec-" + role + "-pid.json"))["pid"]
                for role in ("bridge", "vendor")
            ]
            app.click("aisle_run.stop_run")

            def none_live(_):
                for pid in groups:
                    try:
                        os.killpg(pid, 0)
                        return False
                    except ProcessLookupError:
                        continue
                return True

            app.wait(none_live, "both owned Dora process groups to exit", 30)
            assert (stopped / "stopped.json").exists() and not (
                stopped / "result.json"
            ).exists()
            stopped_groups = len(groups)
        app.click("aisle_run.start_run")
        app.wait(
            lambda s: not s["state"]["busy"]
            and (s["state"]["result"] or {}).get("spec_request_id"),
            "receive completed physics result over SPEC",
            300,
        )
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click("aisle_evidence.device_mode")
        app.click("aisle_evidence.device_replay")
        text("aisle_evidence.device_result", "Simulation skill verified")
    (result_file,) = state.glob("*-run/result.json")
    trial = result_file.parent
    result = read(result_file)
    receipt = read(trial / "spec-receipt.json")
    (denial,) = [
        json.loads(row)
        for row in (trial / "spec-denials.jsonl").read_text().splitlines()
    ]
    assert (
        denial["code"] == "OBSERVATION_DENIED"
        and "simulator_object_poses" in denial["observation_inputs"]
    )
    assert denial["request_id"] != receipt["request_id"]
    (preflight_path,) = state.glob("*-device-preflight/preflight.json")
    preflight = read(preflight_path)
    assert (
        not preflight["physical_qualification"]
        and preflight["motor_commands_sent"] == 0
    )
    assert preflight["spec_simulation"]["receipt_sha256"] == sha(
        trial / "spec-receipt.json"
    )
    if args.robot == "G1":
        assert preflight["state"] == "Dry run measured" and preflight["joints"] == 29
    else:
        assert (
            preflight["state"] == "Adapter incomplete"
            and "measured_odometry" in preflight["missing_capabilities"]
        )
    measurement = read(trial / "spec-measurement.json")
    assert result["task_success"] and measurement["task_success"]
    assert result["spec_receipt_sha256"] == sha(trial / "spec-receipt.json")
    assert receipt["measured_result_sha256"] == sha(trial / "spec-measurement.json")
    assert (
        receipt["package"] == lock["package"] and receipt["runtime"] == lock["runtime"]
    )
    assert result["spec_request_id"] == receipt["request_id"]
    assert (
        sha(prep)
        == sha(trial / "controller.json")
        == receipt["controller_sha256"]
        == result["controller_hash"]
    )
    if args.robot == "G1":
        assert (
            result["wrist_reach"]["passed"]
            and result["disturbance"]["passed"]
            and result["both_feet_loaded_steps"] == 2000
        )
        measures = {
            k: result[k]
            for k in (
                "physics_ticks",
                "both_feet_loaded_steps",
                "wrist_reach",
                "disturbance",
            )
        }
    else:
        assert result["kind"] == "table-transfer" and result["task_success"]
        measures = {
            k: result[k]
            for k in (
                "physics_ticks",
                "wheel_supported_ticks",
                "cubes",
                "actuator_audit",
            )
            if k in result
        }
    (folder / "verification.json").write_text(
        json.dumps(
            dict(
                backend=args.backend,
                robot=args.robot,
                bridge=receipt,
                direct_package_sha256=first.split("SHA-256 ")[-1],
                prepared_controller_sha256=sha(prep),
                result_sha256=sha(result_file),
                verdict=measures,
                changed_main_rejected=read(error_file)["message"],
                privileged_input_rejected=True,
                source_restored=True,
                stop_verified_groups=stopped_groups,
                rejected_request=denial,
                device_preflight=dict(
                    state=preflight["state"],
                    motor_commands_sent=preflight["motor_commands_sent"],
                    receipt_sha256=sha(preflight_path),
                    physical_qualification=preflight["physical_qualification"],
                    missing_capabilities=preflight.get("missing_capabilities", []),
                    replay_steps=preflight.get("dry_run_steps"),
                    max_target_difference_rad=preflight.get(
                        "max_nexus_target_difference_rad"
                    ),
                ),
            ),
            indent=2,
        )
        + "\n"
    )
    print(
        f"{args.robot} executed the selected skill through the SPEC bridge; Nexus verdict passed"
    )


if __name__ == "__main__":
    main()
