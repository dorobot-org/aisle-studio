"""Compile a frozen Studio scene into Nexus bodies and independent task bindings."""

from dataclasses import dataclass
import hashlib
import json
import math

import numpy as np

# The Nano URDF origin is above its wheel contact plane.
FLOOR_Z = -0.0343
PALETTE = {
    "neutral": [0.48, 0.50, 0.52],
    "oak": [0.38, 0.23, 0.12],
    "red": [0.94, 0.10, 0.08],
    "green": [0.10, 0.86, 0.12],
    "blue": [0.10, 0.26, 0.94],
}


@dataclass
class Compiled:
    document: dict
    specs: list
    color: str | None
    cube: str | None
    tabletop: str | None
    surface: dict | None
    sha256: str

    def readback(self, scene, specs, bodies):
        return {
            "task_sha256": self.sha256,
            "scene": {k: self.document["scene"][k] for k in ("id", "revision", "name")},
            **({"ambient_rgb": list(scene.ambient)} if self.document["scene"].get("rules") else {}),
            **(
                dict(
                    pickup_instance=self.document["pickup"],
                    placement=self.document["placement"],
                    cube_body=self.cube,
                    support_body=self.tabletop,
                )
                if self.cube
                else dict(standing=self.document["standing"])
            ),
            "bodies": [
                dict(
                    name=s["name"],
                    instance=s.get("instance"),
                    asset=s.get("asset"),
                    collider=s.get("collider"),
                    physics_body=int(scene.gpu_index(0, b.handles[0])),
                    measured_center_m=b.get_pos().tolist(),
                    measured_quat_wxyz=b.get_quat().tolist(),
                    size_m=s["size"],
                    fixed=s["fixed"],
                    friction=s["friction"],
                    mass_kg=s.get("mass_kg"),
                    restitution=s["restitution"],
                )
                for s, b in zip(specs, bodies, strict=True)
            ],
        }


def vec(value, n):
    a = np.asarray(value, dtype=float)
    if a.shape != (n,) or not np.isfinite(a).all():
        raise ValueError("Invalid scene coordinates")
    return a


def rotate_xy(v, angle):
    c, s = math.cos(angle), math.sin(angle)
    return np.array([v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]])


def intersects(a, b):
    if (
        abs(a["position"][2] - b["position"][2])
        >= (a["size"][2] + b["size"][2]) / 2 - 1e-7
    ):
        return False

    def axes(spec):
        w, _, _, z = spec["quat"]
        c, s = 1 - 2 * z * z, 2 * w * z
        return np.array([[c, s], [-s, c]])

    ax, bx = axes(a), axes(b)
    delta = np.array(a["position"][:2]) - b["position"][:2]
    for direction in np.vstack((ax, bx)):
        extent = (
            np.abs(ax @ direction) @ a["size"][:2]
            + np.abs(bx @ direction) @ b["size"][:2]
        ) / 2
        if abs(delta @ direction) >= extent - 1e-7:
            return False
    return True


def load(path, expected=None, sampled=None):
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if expected and digest != expected:
        raise ValueError("Saved task contents differ from the selected revision")
    doc = json.loads(raw)
    standing = (
        doc.get("schema") == "aisle-studio-g1-standing-v1" and doc.get("robot") == "G1"
    )
    if standing:
        from g1_stand import impact_parameters

        if (sampled or {}).get("controller"):
            raise ValueError("Choose a scene or material variation for G1 standing")
        reach = doc["standing"]
        impact_parameters(reach["impact"])
        if (
            reach["joint"]
            not in ("left_shoulder_pitch_joint", "right_shoulder_pitch_joint")
            or not 0.03 <= abs(reach["delta_rad"]) <= 0.15
        ):
            raise ValueError(
                "Choose a bounded shoulder reach between 0.03 and 0.15 radians"
            )
    elif (
        doc.get("schema") != "aisle-studio-nano-pickup-v1" or doc.get("robot") != "Nano"
    ):
        raise ValueError("Choose a saved Nano pickup or G1 standing-reach task")
    scene = sampled["scene"] if sampled else doc["scene"]
    room = vec(scene["room_m"], 2)
    if min(room) <= 0:
        raise ValueError("Invalid room dimensions")
    assets = {(a["id"], a["revision"]): a for a in doc["assets"]}
    active = {}
    specs = []
    cube = None
    tabletop = None
    placement = None
    chosen_color = None
    cues = {}
    support_cues = {}

    def fixed(name, size, position):
        specs.append(
            dict(
                name=name,
                size=size,
                position=position,
                fixed=True,
                friction=0.7,
                restitution=0.0,
                color=(
                    [0.65, 0.62, 0.57, 1.0]
                    if name == "floor"
                    else [0.80, 0.83, 0.85, 1.0]
                ),
            )
        )

    w, d = map(float, room)
    fixed("floor", [w, d, 0.1], [0, 0, FLOOR_Z - 0.05])
    for name, size, pos in [
        ("north_wall", [w, 0.10, 2.7], [0, (d + 0.10) / 2, FLOOR_Z + 1.35]),
        ("south_wall", [w, 0.10, 2.7], [0, -(d + 0.10) / 2, FLOOR_Z + 1.35]),
        ("east_wall", [0.10, d, 2.7], [(w + 0.10) / 2, 0, FLOOR_Z + 1.35]),
        ("west_wall", [0.10, d, 2.7], [-(w + 0.10) / 2, 0, FLOOR_Z + 1.35]),
    ]:
        fixed(name, size, pos)
    for item in scene["instances"]:
        iid = item["id"]
        if not iid or iid in active:
            raise ValueError("Duplicate scene instance")
        pin = item["asset"]
        asset = assets[(pin["id"], pin["revision"])]
        active[iid] = asset
        pos = vec(item["position_m"], 3)
        yaw = math.radians(item["yaw_deg"])
        if not math.isfinite(yaw):
            raise ValueError("Invalid object orientation")
        p = asset["physics"]
        geom = asset["geometry"]
        boxes = geom["colliders"]
        color = p.get("appearance", {}).get("value", "neutral")
        if color not in PALETTE:
            raise ValueError(f"Unknown appearance on {item['name']}")
        movable = p["movable"]["value"]
        if not boxes:
            raise ValueError(f"Measure the dimensions of {item['name']}")
        if movable and len(boxes) != 1:
            raise ValueError(
                f"Movable compound asset {item['name']} requires a compound body adapter"
            )
        if color in ("red", "blue", "green"):
            cues[color] = cues.get(color, 0) + 1
        mass = p["mass_kg"]["value"] if movable else None
        if movable and (mass is None or not math.isfinite(mass) or mass <= 0):
            raise ValueError("Enter the movable object's mass")
        friction = (
            (sampled or {})
            .get("materials", {})
            .get(iid, {})
            .get("friction", p["friction"]["value"])
        )
        bounce = p["restitution"]["value"]
        if not math.isfinite(friction) or friction < 0 or not 0 <= bounce <= 1:
            raise ValueError("Invalid contact material")
        if geom["surfaces"]:
            support_cues[color] = support_cues.get(color, 0) + 1
        support = not standing and item["id"] == doc["placement"]["instance"]
        surface = (
            next(
                (
                    s
                    for s in geom["surfaces"]
                    if s["name"] == doc["placement"]["surface"]
                ),
                None,
            )
            if support
            else None
        )
        if support and (not surface or movable):
            raise ValueError("Selected fixed placement surface is missing")
        for i, box in enumerate(boxes):
            key = f"instance-{iid}-collider-{i}"
            local = vec(box["center_m"], 3)
            size = vec(box["size_m"], 3)
            if min(size) <= 0:
                raise ValueError("Invalid collider size")
            center = pos + rotate_xy(local, yaw) + [0.0, 0.0, FLOOR_Z]
            ext = (
                abs(math.cos(yaw)) * size[0] + abs(math.sin(yaw)) * size[1],
                abs(math.sin(yaw)) * size[0] + abs(math.cos(yaw)) * size[1],
            )
            if (
                any(abs(center[k]) + ext[k] / 2 > room[k] / 2 + 1e-6 for k in (0, 1))
                or center[2] - size[2] / 2 < FLOOR_Z - 1e-6
            ):
                raise ValueError(
                    f"{item['name']} is outside the room or below the floor"
                )
            # Conservative parked Nano footprint shared with the room-survey importer.
            if (
                abs(center[0]) < ext[0] / 2 + 0.29
                and abs(center[1]) < ext[1] / 2 + 0.31
                and center[2] - size[2] / 2 < FLOOR_Z + (1.55 if standing else 0.40)
            ):
                raise ValueError(
                    f"{item['name']} obstructs the {doc['robot']} start pose"
                )
            specs.append(
                dict(
                    name=key,
                    instance=iid,
                    asset=pin,
                    collider=box["name"],
                    size=size.tolist(),
                    position=center.tolist(),
                    quat=[math.cos(yaw / 2), 0.0, 0.0, math.sin(yaw / 2)],
                    fixed=not movable,
                    friction=friction,
                    restitution=bounce,
                    mass_kg=mass,
                    color=[*PALETTE[color], 1.0],
                )
            )
            if not standing and iid == doc["pickup"]:
                if (
                    not movable
                    or not p["graspable"]["value"]
                    or len(boxes) != 1
                    or color not in ("red", "blue", "green")
                ):
                    raise ValueError(
                        "Select a movable, graspable red, green or blue box"
                    )
                chosen_color, cube = color, key
            if surface:
                sc = vec(surface["center_m"], 3)
                ss = vec(surface["size_m"], 2)
                if min(ss) <= 0:
                    raise ValueError("Invalid placement area")
                if abs(local[2] + size[2] / 2 - sc[2]) < 1e-6 and np.all(
                    abs(sc[:2] - local[:2]) + ss / 2 <= size[:2] / 2 + 1e-6
                ):
                    if tabletop is not None:
                        raise ValueError(
                            "Placement surface intersects multiple collider tops"
                        )
                    tabletop = key
                    placement = dict(
                        appearance=color,
                        center_offset_m=(sc - local).tolist(),
                        size_m=ss.tolist(),
                    )
    if sampled:
        placed = [v for v in specs if v.get("instance")]
        for i, body in enumerate(placed):
            for other in placed[:i]:
                if body["instance"] != other["instance"] and intersects(body, other):
                    raise ValueError(
                        f"Sampled colliders intersect: {body['instance']} and {other['instance']}"
                    )
    if standing:
        return Compiled(doc, specs, None, None, None, None, digest)
    if not cube or not tabletop:
        raise ValueError(
            "Bound pickup or placement surface is missing from this scene revision"
        )
    if cues[chosen_color] != 1:
        raise ValueError(
            "Two instances share the target RGB appearance. Give them distinguishable appearances for this controller."
        )
    if support_cues[placement["appearance"]] != 1:
        raise ValueError(
            "Multiple placement surfaces share the same visual appearance. Select a distinguishable tabletop for this controller."
        )
    return Compiled(doc, specs, chosen_color, cube, tabletop, placement, digest)
