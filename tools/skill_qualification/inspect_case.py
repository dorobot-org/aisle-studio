"""Read one disclosed qualification case and its original onboard release exposure."""

import argparse
from io import BytesIO
import json
import math
import re
import sys
from pathlib import Path

import numpy as np

from cohort import directory_for, digest, encode, read, sha
from review import release_packets


def release_view(case, row, exposure):
    for file, key in [
        ("calibration.json", "calibration_sha256"),
        ("actions.jsonl", "actions_sha256"),
    ]:
        if sha(case / file) != row[key]:
            raise ValueError(
                "The sensor calibration or journal differs from the verified case"
            )
    packets = release_packets(case)
    if digest(encode(packets)) != row["artifacts"]["release_packets_sha256"]:
        raise ValueError("The release exposure differs from the verified case")
    result = {"exposures": len(packets), "exposure_index": exposure}
    if not packets:
        return result, b""
    if not 0 <= exposure < len(packets):
        raise ValueError("Choose a recorded release exposure")
    name = sorted(packets)[exposure]
    tick = int(Path(name).stem)
    raw = (case / "release-observations" / name).read_bytes()
    if digest(raw) != packets[name]:
        raise ValueError("The exposure changed during inspection")
    with np.load(BytesIO(raw), allow_pickle=False) as packet:
        images = [packet[k] for k in ("head", "left_wrist", "right_wrist")]
        height = max(img.shape[0] for img in images)
        pixels = np.concatenate(
            [
                np.pad(img, ((0, height - img.shape[0]), (0, 0), (0, 0)))
                for img in images
            ],
            axis=1,
        )
        stamp = int(packet["stamp_ns"])
    rgb = pixels.tobytes()
    image_id = digest(rgb)
    actions = [
        json.loads(line) for line in (case / "actions.jsonl").read_text().splitlines()
    ]
    action = next(
        a for a in actions if a.get("op") == "release observation" and a["tick"] == tick
    )
    survey = next(
        (
            a
            for a in actions
            if a.get("survey_camera") == "right_wrist" and a["tick"] == tick
        ),
        None,
    )
    caption = (
        f"{action['color'].capitalize()} release · tick {tick} · {stamp / 1e9:.2f} s"
    )
    if survey:
        count = survey["survey_occupants"]
        caption += f" · raised wrist · {count} earlier cube{'s' if count != 1 else ''} observed"
    result.update(
        exposure_index=exposure,
        frame_sha256=image_id,
        width=pixels.shape[1],
        height=pixels.shape[0],
        caption=caption,
    )
    return result, rgb


def case_view(baseline, expected, index, exposure, retry=None, expected_receipt=None):
    report = read(baseline / "verification.json")
    rows = read(baseline / "qualification-rows.json")
    registration = read(baseline / "registration.json")
    if not (
        expected == digest(encode(report))
        and report["rows_sha256"] == digest(encode(rows))
        and report["registration_sha256"]
        == baseline.name
        == digest(encode(registration))
        and registration["manifest_sha256"] == sha(baseline / "suite.json")
        and report["cases"] == registration["cases"] == len(rows)
        and 0 <= index < len(rows)
        and rows[index]["index"] == index
    ):
        raise ValueError("The selected case differs from the reviewed qualification")
    row, case = rows[index], baseline / f"case-{index:03}"
    original = row
    if retry is not None:
        if registration["robot"] != "Nano" or not re.fullmatch(r"[0-9]+-[0-9]+", retry):
            raise ValueError("Choose a saved Nano retry")
        case = (
            baseline.parent.parent
            / "case-replays"
            / baseline.name
            / f"case-{index:03}"
            / retry
        )
        comparison = read(case / "comparison.json")
        if not (
            comparison["original_registration"] == baseline.name
            and comparison["trial"] == retry
            and comparison["case"] == index
            and comparison["baseline_receipt_sha256"]
            == original["spec"]["receipt_sha256"]
            and comparison["candidate_controller_sha256"]
            == sha(case / "controller.json")
            and comparison["candidate_receipt"]["receipt_sha256"] == expected_receipt
            and comparison["candidate_receipt"]["receipt_sha256"]
            != comparison["baseline_receipt_sha256"]
        ):
            raise ValueError("The selected retry differs from the paired record")
        if "evidence" not in comparison:
            raise ValueError("This earlier retry has no retained exposure digests")
        row = comparison["evidence"] | {
            "spec": comparison["candidate_receipt"],
            "opening": original["opening"],
        }
        if (
            sha(case / "result.json") != row["result_sha256"]
            or bool(row["passed"]) != comparison["candidate_passed"]
        ):
            raise ValueError("The retry result differs from the original measurement")
    result = dict(index=index, exposures=0, exposure_index=0)
    rgb = b""
    verdict = "passed" if row["passed"] else "failed"
    if not row["verified"]:
        return result | {
            "summary": f"Case {index} · unverified\n"
            + row.get("issue", "Evidence incomplete")
        }, rgb
    receipt = row["spec"]
    if sha(case / "spec-receipt.json") != receipt["receipt_sha256"]:
        raise ValueError("The case receipt differs from the verified record")
    label = "Disclosed retry" if retry else "Case"
    summary = [f"{label} {index} · {verdict} · {row['guard_bypasses']} guard bypasses"]
    if registration["robot"] == "G1":
        for name in (
            "wrist-trace.npz",
            "impact.json",
            "impact-contacts.json",
            "motor-work.npz",
        ):
            if sha(case / name) != row["artifacts"][name]:
                raise ValueError("The G1 trace differs from the verified case")
        manifest = read(baseline / "suite.json")
        boundary = manifest["boundary_cases"]
        corner_pass = sum(r["verified"] and r["passed"] for r in rows[:boundary])
        interior_pass = sum(r["verified"] and r["passed"] for r in rows[boundary:])
        result["coverage"] = (
            f"{corner_pass}/{boundary} boundary impacts passed · {interior_pass}/{len(rows) - boundary} interior impacts passed"
        )
        rec = row["opening"]["recipe"]
        kind = "Boundary" if index < boundary else "Interior"
        recovery = row["recovery_s"]
        recovery_text = (
            f"{recovery:.2f} s recovery"
            if recovery is not None
            else "no stable recovery"
        )
        summary.extend(
            [
                f"{kind} impact · {rec['mass_kg']:g} kg at {rec['speed_m_s']:g} m/s · lateral offset {rec['lateral_m']:g} m",
                f"{row['loaded_steps']} two-foot-supported steps · {row['wrist_error_mm']:.2f} mm settled wrist error · {recovery_text}",
            ]
        )
        summary.extend(row.get("reasons", []))
    else:
        if retry:
            result["coverage"] = (
                "Development run · candidate "
                + comparison["candidate_controller_sha256"][:16]
            )
        elif report["verified"] == len(rows):
            n, z = len(rows), 1.959963984540054
            fraction = report["successes"] / n
            denom = 1 + z * z / n
            center = (fraction + z * z / (2 * n)) / denom
            radius = (
                z
                * math.sqrt(fraction * (1 - fraction) / n + z * z / (4 * n * n))
                / denom
            )
            result["coverage"] = (
                f"Three-cube success across layouts · 95% Wilson interval {max(0, center - radius):.1%}–{min(1, center + radius):.1%}"
            )
        summary.append(
            f"Layout seed {row['opening']['seed']} · {row['physics_ticks']} scored ticks"
        )
        for color in read(baseline / "suite.json")["color_order"]:
            cube = row["cubes"][color]
            outcome = "passed" if cube["passed"] else "failed"
            summary.append(
                f"{color.capitalize()} {outcome} · {cube['opposing_finger_airborne_ticks']} airborne grip ticks · {cube['table_contact_tail_ticks']} final tabletop contact ticks"
            )
        if row.get("error"):
            summary.append(row["error"].removeprefix("ValueError: "))
        visual, rgb = release_view(case, row, exposure)
        result.update(visual)
    result["summary"] = "\n".join(summary)
    return result, rgb


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--id", required=True)
    parser.add_argument("--expected-verdict", required=True)
    parser.add_argument("--case", type=int, required=True)
    parser.add_argument("--exposure", type=int, default=0)
    parser.add_argument("--retry", help="Saved paired development trial")
    parser.add_argument(
        "--expected-receipt", help="Candidate receipt selected in Studio"
    )
    args = parser.parse_args()
    view, rgb = case_view(
        directory_for(args.state, args.id),
        args.expected_verdict,
        args.case,
        args.exposure,
        args.retry,
        args.expected_receipt,
    )
    sys.stdout.buffer.write(json.dumps(view).encode("utf-8") + b"\n" + rgb)
