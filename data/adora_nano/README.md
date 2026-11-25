# Adora Nano model snapshot

This directory contains the local CAD revision used by DoRobot Studio. It is
self-contained: `adora_nano.urdf`, `adora_nano_floating.xml`, and `meshes/` use
relative paths. No symlink or sibling checkout is required.

The model is based on [DoraCN/AdoraNano](https://github.com/DoraCN/AdoraNano)
commit `43f94e2ae32352464e056f25a4aedb7912136669`, with the supplied October 31
arm CAD update. `provenance.json` records input hashes and the path adaptation;
the right arm preserves that CAD revision. The left arm has the local symmetric
simulation override described below.

The chassis and head retain the upstream meshes. Their comparison with the
October 22 CAD was sampled, not an exact part-by-part match. Masses and inertias
remain nominal. The floating XML is a collision inspection model without
ground, actuators or visual meshes; it has small zero-pose contact overlaps.
The synthetic Studio clips are for viewing joint motion, not hardware control.

Source STEP files and local CAD analysis reports are not included.
Use `../../tools/run_nano_preview.sh` from this directory to open the Studio preview.

Studio uses a display palette of white body panels, orange arms, graphite joints
and blue head-turntable/wrist-housing accents, with dark cameras and gripper fingers.
The base cover is black; the screen is dark grey with a deeper black bezel.
These URDF materials are a visualization choice, not measured hardware colours.

The original `meshes/visual/base_link.stl` is retained. Its triangle records are
partitioned verbatim into `base_body.stl`, `base_cover.stl`, `screen_frame.stl`
and `screen_surface.stl` so each surface can have its own material. The three
coloured parts are visual-only fixed children of `base_link`; the original
articulated joints, collision meshes and inertial properties are not changed by
this surface-colour partition.
`meshes/visual/surface-parts.json` records the source and output hashes.
Regenerate the parts from the repository root with:

```sh
uv run --no-project --with trimesh --with scipy tools/split_nano_surfaces.py
```

## Symmetric simulation assembly

The local model follows the requested **mirror symmetry about `base_link Y=0`**
(the vertical XZ plane). The left mount is at `(0.1111, +0.23, 0.26615)` m and
the right at `(0.1111, -0.23, 0.26615)` m. A 180° yaw of one complete arm is
not a mirror operation; the previous yaw-only override has been replaced.

The source base STEP contains two same-handed, identically oriented arm instances,
460 mm apart. This local override is a requested simulation configuration, **not
a verified reconstruction of the hardware installation**. The right arm is the
reference. For reflection `S = diag(1, -1, 1)`, the left chain uses `S R S`
rotations, `S p` positions and `-S axis` revolute axes, retaining right-handed link
frames. Left visual and collision vertices, centres of mass and inertia tensors
are also reflected; mesh winding is reversed to keep outward normals. The wrist
camera and both gripper fingers are included. J1–J6 names and joint limits match
on both sides. Equal numeric joint values and gamepad inputs produce mirrored
motions; these signs are simulation coordinates, not calibrated servo signs.

The live scene starts with equal left/right arm angles. The floating XML follows
the same reflection. `provenance.json` records the override, and the right-arm
assets provide an unchanged regeneration source:

```sh
uv run --no-project --with numpy tools/mirror_nano_arm.py
```

`tools/aisle-validation/test_nano_symmetry.py` checks every visual/collision mesh,
joint limits, inertia, startup posture and 500 configurations of the full chain.

For a repeatable reach audit, run from the repository root (use a new output
directory each time):

```sh
uv run --project ../AISLE --frozen --python 3.13.5 \
  --with dimforge-nexus3d==0.2.1 \
  python tools/aisle-validation/verify_arm_reach.py \
  --aisle ../AISLE --output tools/aisle-validation/runs/arm-extremes-new
```

The audit samples 65,536 joint configurations, tests three separated candidates
for each of six reach directions, and probes both limits of J1–J6. Arms execute
separately with the other arm parked, using rate-limited motor targets, gravity,
ground and self-collision in 60 isolated worlds stepped together. It compares
mirrored final positions within 5 mm and final joint angles within 1°.
`report.json` distinguishes
symmetry from actually reaching a requested joint limit; collision-blocked targets
are reported explicitly. Transient path deviations, their times and contact
status are separate diagnostics; passing final reach does not imply every point
of the motion agrees. `physics-traces.npz` includes joint commands, observations,
TCP positions, timestamps and contact flags; `extremes.csv` and `workspace.png`
summarize measurements. These are sampled extrema and motor trajectories, not a
proof of the global collision-free workspace or validation of physical hardware.
