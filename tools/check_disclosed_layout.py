"""Drive a paired Nano failure replay from the native qualification page and reopen its result."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil

from studio_native import NativeStudio


def read(path):
    return json.loads(path.read_bytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for field in ("studio", "remote", "session", "baseline", "controller"):
        parser.add_argument("--" + field, required=True, type=Path)
    parser.add_argument("--case", type=int, required=True)
    parser.add_argument(
        "--backend", choices=["offscreen", "headless"], default="headless"
    )
    parser.add_argument("--expect-pass", action="store_true")
    args = parser.parse_args()
    folder = args.session.resolve()
    state = folder / "state/aisle"
    prep = state / "1-Nano-prepare"
    prep.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(args.controller, prep / "controller.json")
    baseline = args.baseline.resolve()
    original = (baseline / "verification.json").read_bytes()
    report = json.loads(original)
    failed_cases = report["failed_cases"]
    selected = failed_cases.index(args.case)
    quals = state / "qualifications"
    quals.mkdir()
    (quals / baseline.name).symlink_to(baseline, target_is_directory=True)
    census = {p.name for p in baseline.iterdir()}
    prefix = "aisle_evidence.qa_"
    app = NativeStudio(args, folder)

    def select_case(index):
        app.click(prefix + "cases")
        for code in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
            app.act("key", "w=0", "c=" + code, "wait=1")
        app.wait(
            lambda s: f"Case {failed_cases[index]} ·"
            in (s["controls"][prefix + "cases"].get("selected_label") or ""),
            "selected disclosed layout",
        )

    def open_case():
        app.click("aisle_task.nano")
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click(prefix + "mode")
        app.control(prefix + "replay_case")
        select_case(selected)

    with app.launch():
        open_case()
        app.click(prefix + "replay_case")
        app.wait(
            lambda s: f"Case {args.case} · baseline failed → candidate "
            in s["controls"].get(prefix + "comparison", {}).get("text", ""),
            "measured case comparison",
            720,
        )
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / "disclosed-layout.png"))
    with app.launch():
        open_case()
        app.wait(
            lambda s: f"Case {args.case} · baseline failed → candidate "
            in s["controls"].get(prefix + "comparison", {}).get("text", ""),
            "restored case comparison",
        )
        if len(failed_cases) > 1:
            select_case((selected + 1) % len(failed_cases))
            app.wait(
                lambda s: not s["controls"].get(prefix + "comparison", {}).get("visible"),
                "previous comparison hidden on a different case",
            )
            select_case(selected)
            app.wait(
                lambda s: s["controls"].get(prefix + "comparison", {}).get("visible")
                and f"Case {args.case} · baseline failed → candidate "
                in s["controls"][prefix + "comparison"]["text"],
                "original comparison restored on reselect",
            )
    directory = state / "case-replays" / baseline.name / f"case-{args.case:03}"
    comparison = read(directory / "latest.json")
    trial = directory / comparison["trial"]
    assert comparison == read(trial / "comparison.json")
    assert comparison["baseline_passed"] is False and comparison["bypasses"] == 0
    assert (
        comparison["candidate_controller_sha256"]
        == hashlib.sha256(args.controller.read_bytes()).hexdigest()
    )
    assert (
        comparison["candidate_receipt"]["request_id"]
        == read(trial / "spec-receipt.json")["request_id"]
    )
    assert comparison["candidate_passed"] == read(trial / "result.json")["task_success"]
    assert (
        comparison["candidate_receipt"]["receipt_sha256"]
        != comparison["baseline_receipt_sha256"]
    )
    if args.expect_pass:
        assert comparison["candidate_passed"]
    assert (
        census == {p.name for p in baseline.iterdir()}
        and original == (baseline / "verification.json").read_bytes()
    )
    evidence = comparison | {
        "backend": args.backend,
        "reopened": True,
        "case_switch_checked": len(failed_cases) > 1,
        "original_verdict_preserved": True,
    }
    (folder / "verification.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(evidence))


if __name__ == "__main__":
    main()
