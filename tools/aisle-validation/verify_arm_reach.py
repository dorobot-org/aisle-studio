"""Compare both Nano arms' extreme reach in an isolated Nexus physics scene.

Samples the joint-bounded workspace, selects separated extreme candidates, then
drives each arm to them using rate-limited motors with gravity and collisions.
The other arm stays parked. Also probes both limits of all six joints.
No simulator state is written during a trial; set_qpos is used only to reset it.
Results are sampled reach measurements, not a proof of a collision-free global
optimum or of physical hardware calibration.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
from pathlib import Path
import sys
import time
import xml.etree.ElementTree as ET

import numpy as np

from nano_living import NAVIGATION_POSTURE
from nano_reach_physics import PhysicsProbe

SUFFIXES = ("j1_yaw", "j2_pitch", "j3_pitch", "j4_pitch", "j5_roll", "j6_gripper")
MIRROR = np.array([1.0, -1.0, 1.0])
DIRECTIONS = {
    "forward": [1, 0, 0],
    "backward": [-1, 0, 0],
    "up": [0, 0, 1],
    "down": [0, 0, -1],
    "outward": [0, -1, 0],
    "inward": [0, 1, 0],
}
FLOOR = -0.0343


def vector(element, key, default="0 0 0"):
    return np.fromstring(
        element.get(key, default) if element is not None else default, sep=" "
    )


def rotation(axis, angle):
    axis = np.asarray(axis) / np.linalg.norm(axis)
    x, y, z = axis
    k = np.array([[0, -z, y], [z, 0, -x], [-y, x, 0]])
    angle = np.asarray(angle)[..., None, None]
    return np.eye(3) + np.sin(angle) * k + (1 - np.cos(angle)) * (k @ k)


def origin(element):
    result = np.eye(4)
    r, p, y = vector(element, "rpy")
    result[:3, :3] = (
        rotation([0, 0, 1], y) @ rotation([0, 1, 0], p) @ rotation([1, 0, 0], r)
    )
    result[:3, 3] = vector(element, "xyz")
    return result


def stl_vertices(path):
    data = path.read_bytes()
    dtype = np.dtype([("n", "<f4", 3), ("v", "<f4", (3, 3)), ("a", "<u2")])
    assert len(data) == 84 + 50 * int.from_bytes(data[80:84], "little"), path
    return np.frombuffer(data, dtype, offset=84)["v"].reshape(-1, 3)


class ArmModel:
    def __init__(self, path):
        self.path = path
        self.root = ET.parse(path).getroot()
        self.joints = {
            side: [
                j
                for j in self.root.findall("joint")
                if j.get("name").startswith(side + "_")
            ]
            for side in ("left", "right")
        }
        self.bounds = {}
        for link in self.root.findall("link"):
            if not link.get("name").startswith(("left_", "right_")):
                continue
            vertices = []
            for c in link.findall("collision"):
                mesh = c.find("geometry/mesh")
                v = stl_vertices(path.parent / mesh.get("filename")) * vector(
                    mesh, "scale", "1 1 1"
                )
                t = origin(c.find("origin"))
                vertices.append(v @ t[:3, :3].T + t[:3, 3])
            if vertices:
                v = np.concatenate(vertices)
                self.bounds[link.get("name")] = (
                    (v.min(0) + v.max(0)) / 2,
                    (v.max(0) - v.min(0)) / 2,
                )

    def poses(self, side, q):
        q = np.atleast_2d(q)
        poses = {"base_link": np.broadcast_to(np.eye(4), (len(q), 4, 4))}
        for joint in self.joints[side]:
            local = np.broadcast_to(origin(joint.find("origin")), (len(q), 4, 4)).copy()
            if joint.get("type") != "fixed":
                index = SUFFIXES.index(joint.get("name")[len(side) + 1 :])
                local[:, :3, :3] = local[:, :3, :3] @ rotation(
                    vector(joint.find("axis"), "xyz"), q[:, index]
                )
            poses[joint.find("child").get("link")] = (
                poses[joint.find("parent").get("link")] @ local
            )
        return poses

    def workspace(self, count, candidates):
        rng = np.random.default_rng(20261114)
        limits = np.array(
            [
                [
                    float(self.root.find(f"joint[@name='right_{s}']/limit").get(k))
                    for k in ("lower", "upper")
                ]
                for s in SUFFIXES
            ]
        )
        q = rng.uniform(limits[:, 0], limits[:, 1], size=(count, 6))
        q[:, 5] = 0.3  # compare reach with the same gripper opening
        # Include the parked configuration; all trials start here.
        q[0] = [NAVIGATION_POSTURE.get("right_" + s, 0) for s in SUFFIXES]
        right, left = self.poses("right", q), self.poses("left", q)
        rp, lp = right["right_tool_mount"][:, :3, 3], left["left_tool_mount"][:, :3, 3]
        mirror_error = float(np.abs(lp - rp * MIRROR).max())
        # Conservative rotated bounding boxes keep candidate arm meshes above
        # the floor. Self-collision is evaluated by the subsequent motor trial.
        valid = np.ones(count, dtype=bool)
        for name, pose in right.items():
            if name in self.bounds:
                c, h = self.bounds[name]
                low = (pose[:, :3, :3] @ c + pose[:, :3, 3])[:, 2] - np.abs(
                    pose[:, 2, :3]
                ) @ h
                valid &= low >= FLOOR + 0.001
        if valid.sum() < candidates * 6:
            raise ValueError("Insufficient above-floor workspace samples")
        selected = {}
        for label, direction in DIRECTIONS.items():
            order = np.argsort(rp @ direction)[::-1]
            picks = []
            for index in order:
                if valid[index] and all(
                    np.linalg.norm(q[index, :5] - q[i, :5]) > 0.8 for i in picks
                ):
                    picks.append(int(index))
                    if len(picks) == candidates:
                        break
            selected[label] = [
                dict(q=q[i].tolist(), right_tcp=rp[i].tolist(), left_tcp=lp[i].tolist())
                for i in picks
            ]
        return (
            dict(
                samples=count,
                above_floor_samples=int(valid.sum()),
                max_mirror_error_m=mirror_error,
                unconstrained_tcp_bounds_m={
                    "left": [lp.min(0).tolist(), lp.max(0).tolist()],
                    "right": [rp.min(0).tolist(), rp.max(0).tolist()],
                },
                candidates=selected,
            ),
            q,
            lp,
            rp,
            valid,
        )


def plot(output, lp, rp, valid, report):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, axes = plt.subplots(1, 3, figsize=(15, 5), layout="constrained")
    for ax, (i, j, title) in zip(
        axes, [(0, 1, "Top: X/Y"), (0, 2, "Side: X/Z"), (1, 2, "Front: Y/Z")]
    ):
        ax.scatter(
            rp[valid][::8, i],
            rp[valid][::8, j],
            s=2,
            alpha=0.12,
            label="Right FK samples",
            color="#1976d2",
        )
        ax.scatter(
            lp[valid][::8, i],
            lp[valid][::8, j],
            s=2,
            alpha=0.12,
            label="Left FK samples",
            color="#f57c00",
        )
        for row in report["extremes"]:
            for side, color, marker in [
                ("left", "#e65100", "o"),
                ("right", "#0d47a1", "x"),
            ]:
                p = np.array(row[side + "_best_tcp_m"])
                ax.scatter(p[i], p[j], s=45, color=color, marker=marker)
        ax.set(title=title, xlabel="XYZ"[i] + " (m)", ylabel="XYZ"[j] + " (m)")
        ax.set_aspect("equal")
        ax.grid(alpha=0.2)
    axes[0].legend(fontsize=8)
    fig.suptitle(
        "Nano paired reach audit: faint = FK samples; solid markers = measured physics reach\nSeparate arm trials; parked other arm; floor + gravity + collision; sampled envelope, not a global optimum",
        fontsize=11,
    )
    fig.savefig(output / "workspace.png", dpi=160)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aisle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--model",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "data/adora_nano/adora_nano.urdf",
    )
    parser.add_argument("--samples", type=int, default=65536)
    parser.add_argument("--candidates", type=int, default=3)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    sys.path.insert(0, str(args.aisle.resolve() / "src"))
    started = time.monotonic()
    model = ArmModel(args.model)
    workspace, q, lp, rp, valid = model.workspace(args.samples, args.candidates)
    (args.output / "workspace-search.json").write_text(json.dumps(workspace, indent=2))
    np.savez_compressed(
        args.output / "workspace.npz", q=q, left_tcp=lp, right_tcp=rp, above_floor=valid
    )
    print(
        json.dumps(
            {
                "phase": "workspace",
                "samples": args.samples,
                "max_mirror_error_m": workspace["max_mirror_error_m"],
            }
        ),
        flush=True,
    )
    plans = []
    for label, candidates in workspace["candidates"].items():
        plans.extend(
            (f"{label}_{i}", np.array(c["q"]), DIRECTIONS[label])
            for i, c in enumerate(candidates)
        )
    parked = np.array([NAVIGATION_POSTURE.get("right_" + n, 0) for n in SUFFIXES])
    for index, suffix in enumerate(SUFFIXES):
        limits = model.root.find(f"joint[@name='right_{suffix}']/limit")
        for bound in ("lower", "upper"):
            target = parked.copy()
            target[index] = float(limits.get(bound))
            plans.append((f"{suffix}_{bound}", target, None))
    sim = PhysicsProbe(args.model, plans)
    sim.run()
    report = dict(
        model_sha256=hashlib.sha256(args.model.read_bytes()).hexdigest(),
        kind="isolated fixed-base actual Nexus motor trials",
        scope="local symmetric simulation model, not hardware validation; sampled extrema, not proof of global collision-free optimum",
        tolerance_mm=5.0,
        joint_tolerance_deg=1.0,
        workspace=workspace,
        extremes=[],
        joint_limits=[],
    )
    report.update(
        joint_names=sim.names,
        physics_dt_s=0.01,
        worlds=sim.count,
        acceptance="mirrored final TCP <= 5 mm, final joints <= 1 degree, sampled directional reach <= 5 mm; transient path error reported separately",
        asset_sha256={
            name: hashlib.sha256((args.model.parent / name).read_bytes()).hexdigest()
            for name in sorted(
                {mesh.get("filename") for mesh in model.root.iter("mesh")}
            )
        },
    )

    def save():
        report["wall_seconds"] = time.monotonic() - started
        (args.output / "report.json").write_text(
            json.dumps(report, indent=2, allow_nan=False)
        )

    for label, candidates in workspace["candidates"].items():
        trials = []
        for index, candidate in enumerate(candidates):
            row = sim.pair(f"{label}_{index}")
            trials.append(row)
            print(
                json.dumps(
                    {
                        "phase": "reach",
                        "name": row["name"],
                        "error_mm": row["mirror_endpoint_error_mm"],
                        "pass": row["symmetry_pass"],
                    }
                ),
                flush=True,
            )
        row = dict(direction=label, trials=trials)
        for side in ("left", "right"):
            best = max((r[side] for r in trials), key=lambda r: r["best_projection_m"])
            row[side + "_best_tcp_m"] = best["best_tcp_m"]
            row[side + "_reach_m"] = best["best_projection_m"]
        row["reach_difference_mm"] = (
            abs(row["left_reach_m"] - row["right_reach_m"]) * 1000
        )
        row["symmetry_pass"] = row["reach_difference_mm"] <= 5 and all(
            r["symmetry_pass"] for r in trials
        )
        report["extremes"].append(row)
        save()
    parked = np.array([NAVIGATION_POSTURE.get("right_" + n, 0) for n in SUFFIXES])
    for index, suffix in enumerate(SUFFIXES):
        limits = model.root.find(f"joint[@name='right_{suffix}']/limit")
        for bound in ("lower", "upper"):
            target = parked.copy()
            target[index] = float(limits.get(bound))
            row = sim.pair(f"{suffix}_{bound}")
            row["joint"] = suffix
            row["bound"] = bound
            for side in ("left", "right"):
                row[side]["requested_joint_error_deg"] = abs(
                    math.degrees(row[side]["actual_rad"][index] - target[index])
                )
                row[side]["limit_reached"] = (
                    row[side]["requested_joint_error_deg"] <= 1.0
                )
            report["joint_limits"].append(row)
            save()
            print(
                json.dumps(
                    {
                        "phase": "joint_limit",
                        "name": row["name"],
                        "error_mm": row["mirror_endpoint_error_mm"],
                        "pass": row["symmetry_pass"],
                    }
                ),
                flush=True,
            )
    report["symmetry_pass"] = workspace["max_mirror_error_m"] < 1e-9 and all(
        r["symmetry_pass"] for r in report["extremes"] + report["joint_limits"]
    )
    report["all_commanded_limits_reached"] = all(
        r[s]["limit_reached"] for r in report["joint_limits"] for s in ("left", "right")
    )
    pairs = [trial for row in report["extremes"] for trial in row["trials"]] + report[
        "joint_limits"
    ]
    report["max_endpoint_mirror_error_mm"] = max(
        row["mirror_endpoint_error_mm"] for row in pairs
    )
    report["max_path_mirror_error_mm"] = max(
        row["max_path_mirror_error_mm"] for row in pairs
    )
    report["all_paths_within_5mm"] = all(row["path_within_5mm"] for row in pairs)
    save()
    arrays = {}
    for i, trace in enumerate(sim.traces):
        for key in ("q", "tcp", "command", "time_s", "contact"):
            arrays[f"trial_{i}_{key}"] = trace[key]
    np.savez_compressed(args.output / "physics-traces.npz", **arrays)
    with (args.output / "extremes.csv").open("w") as file:
        writer = csv.writer(file)
        writer.writerow(
            ["direction", "left_reach_m", "right_reach_m", "difference_mm", "pass"]
        )
        for r in report["extremes"]:
            writer.writerow(
                [
                    r[k]
                    for k in (
                        "direction",
                        "left_reach_m",
                        "right_reach_m",
                        "reach_difference_mm",
                        "symmetry_pass",
                    )
                ]
            )
    plot(args.output, lp, rp, valid, report)
    print(
        json.dumps(
            {
                "complete": True,
                "symmetry_pass": report["symmetry_pass"],
                "all_commanded_limits_reached": report["all_commanded_limits_reached"],
                "wall_seconds": report["wall_seconds"],
            }
        ),
        flush=True,
    )
    return 0 if report["symmetry_pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
