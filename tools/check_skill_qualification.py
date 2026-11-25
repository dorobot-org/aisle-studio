"""Exercise native cohort registration, measured verification and signed simulation handoff."""

import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
from tempfile import TemporaryDirectory

from studio_native import NativeStudio, ROOT


def read(path):
    return json.loads(path.read_bytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("studio", "remote", "session"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument(
        "--backend", choices=("headless", "offscreen"), default="headless"
    )
    parser.add_argument("--robot", choices=("Nano", "G1"), required=True)
    parser.add_argument(
        "--source",
        type=Path,
        help="Existing raw registered cohort to replay after testing registration",
    )
    args = parser.parse_args()
    folder = args.session.resolve()
    state = folder / "state/aisle"
    fixture = (
        ROOT
        / "docs/product/aisle-studio/environment/skill-qualification"
        / args.robot.lower()
        / "release-2"
    )
    prep = state / ("1-" + args.robot + "-prepare")
    prep.mkdir(parents=True, exist_ok=False)
    for file in ["controller.json", "bound-task.json"]:
        if (fixture / file).exists():
            shutil.copy2(fixture / file, prep / file)
    cohorts = state / "qualifications"
    cohorts.mkdir()
    source = None
    if args.source:
        source = cohorts / args.source.name
        shutil.copytree(
            args.source,
            source,
            ignore=shutil.ignore_patterns(
                "operation.lock",
                "verification.json",
                "qualification-rows.json",
                "reviews.json",
                "handoff-*.json",
            ),
        )
    # Independent ephemeral acceptance identity. This has no trust on an operator's installation.
    signing_dir = TemporaryDirectory(prefix="aisle-review-test-key-")
    key = Path(signing_dir.name) / "acceptance-key"
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True
    )
    trust = folder / "allowed_signers"
    trust.write_text("devhub-test " + key.with_suffix(".pub").read_text())
    os.environ.update(
        DOROBOT_QUALIFICATION_ALLOWED_SIGNERS=str(trust),
        DOROBOT_QUALIFICATION_SIGNING_KEY=str(key),
    )
    app = NativeStudio(args, folder)
    prefix = "aisle_evidence.qa_"

    def field(name, text):
        app.click(prefix + name)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + text, "wait=1")

    def wait_text(name, content, timeout=60):
        return app.wait(
            lambda s: content in s["controls"].get(prefix + name, {}).get("text", ""),
            name + " to show " + content,
            timeout,
        )

    def open_page():
        app.click("aisle_task." + args.robot.lower())
        app.click("tabs.plugin_tab_aisle_evidence")
        app.click(prefix + "mode")
        app.control(prefix + "register")

    with app.launch():
        open_page()
        app.click(prefix + "register")
        app.wait(
            lambda _: len(list(cohorts.glob("*/registration.json")))
            == (2 if source else 1),
            "native preregistration",
        )
        (registered,) = (p for p in cohorts.iterdir() if p != source)
        wait_text("identity", registered.name)
        directory = source or registered
        if source:
            assert registered.name != source.name
            assert (
                registered.name[:16]
                in app.control(prefix + "selector")["selected_label"]
            )
            index = 1
            app.click(prefix + "selector")
            for keycode in ["Home", *(["ArrowDown"] * index), "ReturnKey"]:
                app.act("key", "w=0", "c=" + keycode, "wait=1")
            wait_text("identity", source.name)
            app.click(prefix + "verify")
        else:
            app.click(prefix + "run")
        wait_text("decision", "Awaiting", 7200)
        report = read(directory / "verification.json")
        rows = read(directory / "qualification-rows.json")
        assert len(rows) == report["cases"] and all(r["verified"] for r in rows), report
        assert len({r["spec"]["request_id"] for r in rows}) == len(rows)
        if args.backend == "offscreen":
            sizes = []
            for width, height in [(1024, 768), (1440, 900)]:
                app.act("resize", "window=0", f"w={width}", f"h={height}")
                image = folder / f"skill-review-{width}.png"
                app.act("grab", "w=0", "out=" + str(image))
                assert struct.unpack(">II", image.read_bytes()[16:24]) == (
                    width,
                    height,
                )
                rects = {}
                for name in ["selector", "result", "reviewer", "reason", "reject"]:
                    rect = app.control(prefix + name)["rect"]
                    x, y, w, h = rect
                    assert (
                        -1 <= x
                        and -1 <= y
                        and x + w <= width + 1
                        and y + h <= height + 1
                    ), (name, rect)
                    rects[name] = rect
                sizes.append(
                    {"size": [width, height], "image": image.name, "controls": rects}
                )
            (folder / "window-sizes.json").write_text(
                json.dumps(sizes, indent=2) + "\n"
            )
        if not report["qualified"]:
            assert not read(app.report)["controls"][prefix + "approve"]["enabled"]
        action = "approve" if report["qualified"] else "reject"
        field("reviewer", "untrusted-test")
        field(
            "reason", "Automated signature-path check on isolated simulation evidence"
        )
        app.click(prefix + action)
        wait_text("status", "Reviewer signature failed", 180)
        assert not (directory / "reviews.json").exists()
        field("reviewer", "devhub-test")
        app.click(prefix + action)
        wait_text("decision", "Signed " + action, 180)
        exported = None
        if report["qualified"]:
            app.click(prefix + "export")
            wait_text("status", "handoff-", 180)
            (dossier,) = directory.glob("handoff-*.json")
            exported = read(dossier)
            assert (
                exported["verification"] == report
                and exported["physical_qualification"] is False
            )
        (folder / "review-before-restart.json").write_text(
            json.dumps(read(directory / "reviews.json"))
        )
    with app.launch():
        open_page()
        wait_text("identity", directory.name)
        wait_text("decision", "Signed " + action)
        measurement = directory / "case-000/spec-measurement.json"
        original = measurement.read_bytes()
        try:
            measurement.write_bytes(original + b" ")
            app.click(prefix + "verify")
            wait_text("status", "differs from the signed history", 180)
        finally:
            measurement.write_bytes(original)
        field("reviewer", "devhub-test")
        field("reason", "End of isolated workflow test")
        app.click(prefix + "reject")
        wait_text("decision", "End of isolated workflow test", 180)
        assert not read(app.report)["controls"][prefix + "export"]["enabled"]
        decisions = read(directory / "reviews.json")
        assert len(decisions) == 2 and decisions[1]["decision"]["previous"]
    signing_dir.cleanup()
    artifact = dict(
        robot=args.robot,
        backend=args.backend,
        cases=report["cases"],
        successes=report["successes"],
        verified=report["verified"],
        guard_bypasses=report["guard_bypasses"],
        qualified=report["qualified"],
        registered_via_native=True,
        execution="reopened raw cohort" if source else "launched via native control",
        registration_sha256=directory.name,
        rows_sha256=report["rows_sha256"],
        package_sha256=report["package_sha256"],
        reviewer_fixture="ephemeral devhub-test",
        unknown_reviewer_rejected=True,
        modified_measurement_rejected=True,
        decision_reopened=True,
        signed_handoff_checked=bool(exported),
        final_disposition="reject",
        physical_qualification=False,
    )
    (folder / "verification.json").write_text(json.dumps(artifact, indent=2) + "\n")
    print(json.dumps(artifact))


if __name__ == "__main__":
    main()
