"""The sole Nexus owner in the Nano Dora graph. No trajectory or file writer."""

from __future__ import annotations

import importlib.metadata
from pathlib import Path

import numpy as np

from nano_cameras import create_rig, read_rig
from nano_validation import digest, native_indices
from nano_workspace import add_workspace, capture_objects


class NanoSimulation:
    def __init__(self, config, model):
        from aisle.sim.nexus_backend import _ensure_nexus, _new_scene

        self.config, self.model, self.tick = config, model, 0
        engine = _ensure_nexus(config["backend"])
        self.scene = _new_scene(
            engine, {"sim": {"dt": 0.01, "gravity": [0, 0, -9.81]}}, 1, [0.5] * 3
        )
        self.robot = self.scene.add_urdf_robot(
            Path(config["model"]), "adora_nano", True
        )
        self.scene.add_box("floor", [2, 2, 0.1], [0, 0, -0.0843], fixed=True)
        self.objects, self.bodies = (
            add_workspace(self.scene) if config["workspace"] else ([], [])
        )
        self.rig, self.camera_specs = create_rig(self.scene, self.robot)
        self.scene.build()
        ordered = sorted(
            (j.dofs_idx_local[0], j.name) for j in self.robot.joints if j.n_dofs
        )
        if any(j.n_dofs != 1 for j in self.robot.joints if j.n_dofs):
            raise ValueError("Expected one DOF per movable Nano joint")
        self.indices = native_indices(model["names"], [name for _, name in ordered])
        self.grippers = [
            model["names"].index(side + "_j6_gripper") for side in ("left", "right")
        ]
        self.native_grippers = [self.indices[i] for i in self.grippers]

    @property
    def stamp(self):
        return self.tick * 10_000_000

    def step(self, count=1):
        for _ in range(count):
            self.scene.step()
            self.tick += 1

    def state(self):
        q = np.asarray(self.robot.get_qpos(), dtype=np.float32)[self.indices]
        if q.shape != (17,) or not np.isfinite(q).all():
            raise ValueError("Invalid Nexus joint state")
        return q

    def command(self, target):
        native = np.empty(17, dtype=np.float32)
        native[self.indices] = target
        self.robot.control_dofs_position(native)

    def reset(self):
        self.robot.set_qpos(np.zeros(17, dtype=np.float32))
        self.command(np.zeros(17, dtype=np.float32))
        for spec, body in zip(self.objects, self.bodies):
            body.set_pos(spec["position"])
            body.set_quat([1, 0, 0, 0])
            body.zero_all_dofs_velocity()

    def frame(self, target):
        # Preserve real observation-before-action ordering for AISLE export.
        self.step()
        self.robot.control_dofs_position(
            [target[i] for i in self.grippers], dofs_idx_local=self.native_grippers
        )
        yield "gripper_applied", [target[i] for i in self.grippers], self.stamp
        self.step()
        yield "joint_state", self.state(), self.stamp
        yield (
            "objects",
            capture_objects(self.objects, self.bodies, self.stamp),
            self.stamp,
        )
        for name, arrays in read_rig(self.rig, self.camera_specs, self.stamp):
            yield name, arrays, self.stamp
        self.step()
        self.command(target)
        yield "applied", target, self.stamp
        self.step(7)
        after = self.state()
        excess = max(
            max(low - float(q), float(q) - high, 0.0)
            for name, q in zip(self.model["names"], after)
            for low, high in [self.model["limits"][name]]
        )
        yield (
            "frame_done",
            dict(
                after=after.tolist(),
                tracking_error=float(np.max(np.abs(after - target))),
                limit_excess=excess,
            ),
            self.stamp,
        )

    def info(self):
        c, model = self.config, self.model
        manifest = dict(
            embodiment="adora_nano",
            sim_engine="nexus",
            sim_backend=c["backend"],
            control_transport="dora-rs",
            graph="nano_dora.yaml",
            command_stream="nano_sim__joint_cmd_safe.arrow",
            evidence_kind="simulated_joint_motion_software_fixture",
            hardware_connected=False,
            fixed_base=True,
            head_and_wheels="zero position targets",
            state_joint_names=model["names"],
            units="radians",
            physics_dt_s=0.01,
            capture_fps=10,
            cameras=self.camera_specs,
            scene_objects=self.objects,
            motion="multi-stage free-space arm sweep"
            if c["workspace"]
            else "small joint pulse",
            segmentation_id_map={
                str(k): v for k, v in self.scene.segmentation_idx_dict.items()
            },
            segmentation_entities=[e.name for e in self.scene.entities]
            + [self.robot.name],
            segmentation_robot_links=[link.name for link in self.robot.links],
            camera_rig="nano",
            capture_policy="Every 10 Hz frame crosses Dora; recorder acknowledgement gates next turn",
            source_model=c["model"],
            model_sha256=digest(Path(c["model"])),
            nexus_version=importlib.metadata.version("dimforge-nexus3d"),
            aisle_modules={
                p: digest(Path(c["aisle"]) / p)
                for p in (
                    "src/aisle/sim/nexus_backend.py",
                    "src/aisle/sim/nexus_physics.toml",
                    "src/aisle/harness/trace_recorder.py",
                    "tools/demo_dataset.py",
                )
            },
            command_validation="Dora guard: schema, joint order, finite values, position/velocity limits, held joints and sequential turn IDs",
            model_dynamics="nominal inertias; AISLE default URDF gains/damping; not hardware calibrated",
            dora_runtime=c["runtime"],
            node_sources_sha256=c["sources"],
        )
        contract = dict(
            embodiment="adora_nano",
            state=dict(names=model["names"], units=["rad"] * 17, frame="joint"),
            forwarded=dict(
                names=model["names"],
                units=["rad"] * 17,
                frame="joint",
                semantics="position_target",
            ),
            gripper=dict(
                names=[model["names"][i] for i in self.grippers],
                units=["rad"] * 2,
                frame="joint",
                semantics="position_target",
            ),
            training_action="full_forwarded_position_targets_17",
        )
        return dict(manifest=manifest, contract=contract)
