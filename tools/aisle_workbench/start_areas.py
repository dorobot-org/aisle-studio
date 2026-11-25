"""Realize scene-authored start regions against the frozen collider geometry."""

import math
import random

import numpy as np

from authored_scene import rotate_xy, vec


def sample_areas(scene, assets, selected, random_seed, occupied):
    if not isinstance(selected, list) or not all(isinstance(v, str) for v in selected):
        raise ValueError("Choose scene instances with a start area")
    if len(set(selected)) != len(selected):
        raise ValueError("A start area can only be enabled once per instance")
    items = {item["id"]: item for item in scene["instances"]}
    library = {(a["id"], a["revision"]): a for a in assets}

    def geometry(item):
        pin = item["asset"]
        return library[(pin["id"], pin["revision"])]["geometry"]

    def place(identifier):
        obj = items.get(identifier)
        if obj is None or not obj.get("start_area"):
            raise ValueError(f"Missing start area for {identifier}")
        if any((identifier, axis) in occupied for axis in range(3)):
            raise ValueError(f"Another feature sets the position of {obj['name']}")
        area = obj["start_area"]
        support = area["support"]
        center = np.zeros(3)
        surface_yaw = 0.0
        size = vec(scene["room_m"], 2)
        if support:
            parent_id = support["instance"]
            parent = items.get(parent_id)
            if parent is None:
                raise ValueError(f"Missing support instance for {obj['name']}")
            if parent_id in pending:
                return False
            geom = geometry(parent)
            surface = next(
                (s for s in geom["surfaces"] if s["name"] == support["surface"]), None
            )
            if surface is None:
                raise ValueError(f"Missing named support surface for {obj['name']}")
            local = vec(surface["center_m"], 3)
            size = vec(surface["size_m"], 2)

            def supports_surface(box):
                offset = local - vec(box["center_m"], 3)
                half = vec(box["size_m"], 3) / 2
                return abs(offset[2] - half[2]) < 1e-6 and np.all(
                    np.abs(offset[:2]) + size / 2 <= half[:2] + 1e-7
                )

            if not any(supports_surface(box) for box in geom["colliders"]):
                raise ValueError(
                    f"Named support surface for {obj['name']} lacks a physical collider top"
                )
            surface_yaw = math.radians(parent["yaw_deg"])
            center = vec(parent["position_m"], 3) + rotate_xy(local, surface_yaw)
        bounds = area.get("bounds_m")
        lo, hi = (
            (-size / 2, size / 2)
            if bounds is None
            else (vec(bounds[0], 2), vec(bounds[1], 2))
        )
        if (
            np.any(lo >= hi)
            or np.any(lo < -size / 2 - 1e-7)
            or np.any(hi > size / 2 + 1e-7)
        ):
            raise ValueError(
                f"Start area for {obj['name']} is outside the support or has invalid bounds"
            )
        yaw = math.radians(obj["yaw_deg"]) - surface_yaw
        c, s = abs(math.cos(yaw)), abs(math.sin(yaw))
        minimum, maximum = [], []
        for box in geometry(obj)["colliders"]:
            local = rotate_xy(vec(box["center_m"], 3), yaw)[:2]
            x, y, _ = vec(box["size_m"], 3) / 2
            half = np.array([c * x + s * y, s * x + c * y])
            minimum.append(local - half)
            maximum.append(local + half)
        if not minimum:
            raise ValueError(f"Measure the geometry of {obj['name']}")
        lo = lo - np.min(minimum, axis=0)
        hi = hi - np.max(maximum, axis=0)
        if np.any(hi < lo - 1e-7):
            raise ValueError(
                f"Object footprint exceeds the start area for {obj['name']}"
            )
        rng = random.Random(random_seed(identifier))
        xy = [rng.uniform(a, max(a, b)) for a, b in zip(lo, hi)]
        obj["position_m"] = (center + rotate_xy([*xy, 0], surface_yaw)).tolist()
        obj["support"] = support
        realized[identifier] = dict(
            name=obj["name"],
            support=support,
            local_xy_m=xy,
            position_m=obj["position_m"],
        )
        return True

    pending = set(selected)
    realized = {}
    while pending:
        count = len(pending)
        for identifier in sorted(pending):
            if place(identifier):
                pending.remove(identifier)
        if len(pending) == count:
            raise ValueError("Start-area supports form a cycle")
    return realized
