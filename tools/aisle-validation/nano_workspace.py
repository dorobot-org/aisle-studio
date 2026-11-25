"""Foreground physics props and a visible arm sweep (not a grasp policy)."""

import math

import numpy as np


def add_workspace(scene):
    specs = [
        dict(
            name="workbench",
            size=[0.66, 0.96, 0.04],
            position=[0.57, 0, 0.14],
            color=[0.20, 0.25, 0.32, 1],
            fixed=True,
        )
    ]
    colors = [
        [0.95, 0.16, 0.12, 1],
        [0.10, 0.65, 0.95, 1],
        [0.98, 0.69, 0.08, 1],
        [0.12, 0.78, 0.38, 1],
        [0.66, 0.28, 0.92, 1],
        [0.96, 0.37, 0.12, 1],
    ]
    for i, color in enumerate(colors):
        specs.append(
            dict(
                name=f"cube_{i + 1}",
                size=[0.055] * 3,
                position=[0.43 + 0.20 * (i // 3), 0.23 - 0.23 * (i % 3), 0.20],
                color=color,
                fixed=False,
            )
        )
    bodies = [
        scene.add_box(
            s["name"],
            s["size"],
            s["position"],
            fixed=s["fixed"],
            color=s["color"],
            friction=0.8,
            density=500,
        )
        for s in specs
    ]
    return specs, bodies


def workspace_target(model, mode, phase):
    if (
        mode not in ("left", "right", "both")
        or not math.isfinite(phase)
        or not 0 <= phase <= 1
    ):
        raise ValueError("Invalid motion or phase")
    # Smooth reach, lower, close, lift, sweep, open, return. The gripper cycles
    # in free space; there is no attachment or claim that it picks up a cube.
    points = np.array(
        [
            [0, 0, 0, 0, 0, 0],
            [0.10, 0.22, 0.20, -0.15, 0.10, 0.15],
            [0.16, 0.38, 0.32, -0.32, 0.0, 0.05],
            [0.16, 0.38, 0.32, -0.32, 0.0, 0.65],
            [-0.16, 0.10, -0.15, 0.20, -0.20, 0.65],
            [-0.32, -0.12, 0.12, 0.32, -0.30, 0.65],
            [-0.32, -0.12, 0.12, 0.32, -0.30, 0.08],
            [0, 0, 0, 0, 0, 0],
        ]
    )
    progress = phase * (len(points) - 1)
    index = min(int(progress), len(points) - 2)
    t = progress - index
    t = t * t * (3 - 2 * t)
    arm = points[index] * (1 - t) + points[index + 1] * t
    target = []
    for name in model["names"]:
        value = 0.0
        for side in ("left", "right"):
            if name in model[side] and mode in (side, "both"):
                joint = int(name.split("_j")[1][0]) - 1
                value = float(arm[joint])
                if side == "right" and joint in (0, 4):
                    value = -value
        low, high = model["limits"][name]
        if not low <= value <= high:
            raise ValueError(f"Target outside limits: {name}")
        target.append(value)
    return target


def capture_objects(specs, bodies, stamp):
    from aisle.sim.nexus_backend import quat_wxyz_to_matrix

    transforms = []
    for body in bodies:
        matrix = np.eye(4)
        matrix[:3, :3] = quat_wxyz_to_matrix(np.asarray(body.get_quat()))
        matrix[:3, 3] = np.asarray(body.get_pos())
        if not np.isfinite(matrix).all():
            raise ValueError("Invalid rigid-body pose")
        transforms.append(matrix.flatten(order="F").tolist())
    return dict(sim_time_ns=stamp, transforms=transforms)
