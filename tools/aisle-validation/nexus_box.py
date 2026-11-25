"""Create an AISLE/Nexus box with the authored mass, pose and contact material."""

import math


def add_scene_box(scene, spec, settings):
    size = spec["size"]
    friction = spec.get(
        "friction",
        settings.get("floor_friction", 0.7) if spec["name"] == "floor" else 0.7,
    )
    mass = spec.get("mass_kg")
    density = mass / math.prod(size) if mass is not None else 500.0
    quat = spec.get("quat", [1.0, 0.0, 0.0, 0.0])
    if not spec.get("restitution"):
        return scene.add_box(
            spec["name"],
            size,
            spec["position"],
            quat_wxyz=quat,
            fixed=spec["fixed"],
            friction=friction,
            density=density,
            color=spec["color"],
        )
    # The AISLE box helper has no restitution argument. Use its native Nexus primitives.
    from aisle.sim.nexus_backend import NexusEntity

    nx = scene.nexus3d
    w, x, y, z = quat
    pose = nx.Pose.from_parts(nx.Vec3(*spec["position"]), nx.Quat.from_xyzw(x, y, z, w))
    body = (
        (
            nx.RigidBodyBuilder.fixed()
            if spec["fixed"]
            else nx.RigidBodyBuilder.dynamic()
        )
        .pose(pose)
        .build()
    )
    collider = (
        nx.ColliderBuilder.cuboid(*(v / 2.0 for v in size))
        .friction(friction)
        .friction_combine_rule(scene.friction_combine_rule)
        .density(density)
        .restitution(spec["restitution"])
        .build()
    )
    handle = scene.state.insert_rigid_body_in(0, body, collider, nx.RbdCoupling.NONE)
    scene._register_box_visual(
        handle, collider, size, spec["color"], None, spec["name"]
    )
    entity = NexusEntity(
        scene, len(scene.entities), spec["name"], [handle], spec["fixed"]
    )
    scene.entities.append(entity)
    return entity
