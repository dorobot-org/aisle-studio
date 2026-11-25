# Validate Studio with Adora Nano, Nexus, and AISLE

This workflow controls Adora Nano's two arms through **five independent Dora
nodes**, simulates them in **Nexus**, records all three cameras using AISLE's
trace schema and offline exporter, and converts the result for Studio playback.

The helpers in this directory are an integration probe. They use AISLE's existing
Python components; they do **not** add `adora_nano` to `harness rollout` or modify
AISLE's frozen scenes. A physical robot is not required.

```text
nano_task → nano_trajectory → nano_limits → nano_sim (Nexus)
                 │                │             │
                 └────────────────┴─────────────┤
                                               ▼
                                         nano_recorder ──ack──► nano_task
                                               │
                             AISLE Arrow + raw sensor arrays
                                               │
                                AISLE export → LeRobot → Studio
```

[`nano_dora.yaml`](nano_dora.yaml) declares the actual graph. `nano_sim` alone
owns the physics scene; its only command input comes from `nano_limits`.
The trajectory node generates the existing scripted motion. The guard checks
the Arrow float32 contract, joint order, radians, finite values, position and
velocity limits, held joints, and monotonically ordered request IDs. It rejects
invalid commands rather than silently clipping them.

Every sensor frame crosses Dora as an Arrow byte array containing lossless NPZ
data. The recorder acknowledges a turn only after receiving its proposed and
approved commands, applied-command confirmations, joint state, object poses,
all three cameras and physics completion. Queues use bounded backpressure;
physics cannot run ahead of recording. A failed node aborts the collector, and
an incomplete run is never exported as successful. Runs retain an immutable
copy of the executed node sources, runtime version, process IDs and message
hashes. This is a development graph, not AISLE benchmark attestation.

## Mobile Nano in a living room

For the app-managed workflow, choose **Robot setup → Simulation → Start
simulation**, then **Operate → Record → Stop → Save → Replay**. The following
CLI instructions are for running an external source. See the
[Studio workflow guide](../../docs/FUNCTIONAL_REVIEW.md) for setup and calibration.

`run_living.py` adds a separate mobile simulation to the fixed-base arm probe
below. It procedurally builds a **12 × 10 m** room with walls, a rigid sofa, a
coffee table and **12 free cubes** using AISLE's Nexus backend. The actual Nano
URDF has a floating root. A footprint-aware A* planner uses measured base pose
to drive wheel-joint targets through the five-process
[`nano_living_graph.yaml`](nano_living_graph.yaml):

```text
living_nav → living_guard → living_sim (Nexus, 100 Hz physics)
     ▲                           │
     └── recording ack ─ living_recorder → lossless sources + LeRobot
                                 │
                        living_preview → Unix socket → Studio JIT
```

The simulator never writes root poses during navigation. The first goal pushes
a free cube through bumper contact; subsequent goals lead around the table and
sofa. This is **known-map, ground-truth-pose navigation**, not camera-based SLAM
or grasping. Furniture is rigid. The URDF collision wheels are solid shapes;
forward/turn gains are calibrated in this simulation, and lateral omni-wheel
motion has not been validated. Cameras use the nominal optics described below.
This is an external integration using AISLE components, not a registered AISLE
benchmark scene or a photorealistic room generator.

Before capture, joint motors smoothly reach an observation posture: the head
faces the room interior and the folded wrists point nearer the horizon. The
optical mounts stay fixed to their original links. Five floor cubes are spaced
along the outside of the route so wrist views can observe nearby objects.
During navigation, the controller finishes sharp turns before driving forward.

Sensor images render the URDF's **visual STL meshes and named RGBA materials**,
including orange arms, blue accents and black cover/display parts. Physics
continues to use the original collision meshes. `robot_visuals` in the scene
manifest records the applied palette and hashes of all 28 visual meshes.

Start Studio first, then the graph in another terminal (from this repository):

```sh
tools/run_jit.sh --video --screen live
```

Choose **Robot setup → More → External stream**, enter `/tmp/nano-living.sock` in **Dora source**,
and click **Connect**. The source can start later: Studio waits and reconnects
automatically. **Reconnect** also applies an edited source path. **Disconnect**
stops this viewer's subscription; simulation and recording keep running.
The header shows the measured preview rate and time since the last received
frame. A feed with no fresh frames for three seconds changes to **Waiting for
stream**. This source uses a local Unix-domain socket on the same machine.

Live defaults to the robot and three camera panes. Signals, panel rearrangement,
maximization, and the independent **Live window** remain available. Live and
Replay have separate saved panel layouts; switching modes preserves the replay
episode and playhead. The CLI shortcut `--live-socket /tmp/nano-living.sock`
still connects directly. Restoring an app session never starts a connection.

### DualSense / DualShock gamepad in Operate

Pair the controller in macOS Bluetooth settings. Start a manual simulation:

```sh
uv run --project ../AISLE --frozen --python 3.13.5 \
  --with 'dora-rs==1.0.1' --with 'dimforge-nexus3d==0.2.1' \
  --with 'opencv-python-headless==5.0.0.93' --with 'pygame==2.6.1' \
  python -B tools/aisle-validation/run_teleop.py \
  --aisle ../AISLE --output tools/aisle-validation/runs/gamepad-001 \
  --socket /tmp/nano-gamepad.sock
```

Choose **Robot setup → More → External stream** and connect `/tmp/nano-gamepad.sock`. **Operate** shows the
front camera in the main area and both arm cameras plus
a top-down trace map and a **3D joints** inspection panel in a separate row above
it, without overlap. Click a camera or map preview
to swap it into the main area; click the front preview to return to driving.
The blue trace uses recently received base positions (up to 1,200 preview frames),
not the planned route. All three feeds continue streaming while switching.
Active mode, movement state, and the control mapping remain visible. Operate
has its own saved layout; signals, maximize, alternate panel arrangements, and
the independent monitor remain available. The 3D panel mirrors the streamed
joint angles; blue markers identify the selected control group and yellow
markers/materials identify changing commanded targets while movement is enabled.
A held target at a joint limit does not turn yellow. Drag the 3D model to orbit,
scroll to zoom, and use its maximize button for detailed inspection. New frames preserve the chosen view. **Robot setup** returns to configuration
without stopping the simulation. **Disconnect** on the external stream bar
ends only the viewer subscription; it does not stop an external producer.

- Hold **L1** to enable movement; release to hold the measured pose.
- Release L1, center the sticks, then tap **R1** to cycle Drive → Left arm →
  Right arm → Head. The inactive arm holds its target.
- Drive: left stick up/down moves forward/back; right stick left/right turns.
  D-pad up/down also drives; D-pad left/right turns. L1 alone enables input
  without moving the robot. Changing modes while L1 is held stops the command
  and requires releasing L1 before movement can resume.
  Lateral omni-wheel motion is not enabled in this collision model.
- Arm: left stick controls joints 1/2; right stick controls joints 4/3;
  D-pad left/right controls joint 5; Cross/Circle jog the gripper.
- Head: left stick pans/tilts. **Options** toggles movement pause; release L1
  before resuming.

Simulation teleoperation starts with **recording off**. Its six Dora processes
are gamepad → mapping → guard → physics, plus preview and session recorder.
App-managed sessions expose Record/Stop/Save/Discard in Studio; external CLI
sessions require the corresponding session command file. The autonomous
navigator is absent. SDL's normalized controller API handles PlayStation axes,
buttons and D-pad (DualSense exposes the D-pad as buttons, not a joystick hat).
Complete samples, including neutral and button-only input, are emitted at a
requested 50 Hz. A simulator-side 250 ms watchdog stops target motion if input
expires, and requires releasing L1 after an armed timeout. Connection with L1
already held cannot arm motion until release and neutral input are observed.

Joint velocities, acceleration and URDF position limits are bounded. The physics
step is 10 ms, with cameras every 20 steps; actual wall rates depend on rendering
load and are reported in `progress.json`. No claim of 30 fps is made. The latest
`controller.json`, `command.json` and `progress.json` expose input, guarded output
and actual joints/base pose without accumulating camera archives. Ctrl+C stops
the supervisor and its Dora processes. Choose a new output directory on restart.

```sh
uv run --project ../AISLE --frozen --python 3.13.5 \
  --with 'dora-rs==1.0.1' --with 'dimforge-nexus3d==0.2.1' \
  --with 'opencv-python-headless==5.0.0.93' \
  python -B tools/aisle-validation/run_living.py \
  --aisle ../AISLE --output tools/aisle-validation/runs/living-001 \
  --socket /tmp/nano-living.sock
```

Use an unused output directory and socket path. Install Dora CLI **1.0.1** on
`PATH`; the runner checks it against the Python API. `--no-wait-viewer` permits
recording without Studio. `--goals '[[1.25,0],[2.2,1.6],[2.2,3.9],[-2.2,3.9]]'`
overrides goals; `--seed` changes cube placement. A timeout or failed node aborts
the run and preserves diagnostics. A truncated `--max-frames` run exports its
data but fails navigation acceptance.

For an ongoing Live view, add **`--continuous --preview-only`**. The robot
reverses the waypoint route at each end and keeps simulating without resetting
its pose or stopping after the first 74-second route. Studio reconnects to the
new session automatically. This mode shows **Recording off** and writes only
the latest progress and scene metadata, so leaving it open does not accumulate
camera archives. Stop the producer with Ctrl+C. `--max-frames N` or `--timeout S`
can bound a test; both default to unlimited in continuous mode. Finite recording
runs keep the full RGB/depth/segmentation archive described below.

Physics and contact auditing run at **100 Hz**; synchronized cameras and robot
state are captured at **5 Hz simulation time**, 640 × 480 by default. This is
not a claim of 5 fps wall-clock throughput. The recorder applies backpressure
instead of dropping source frames; at most two unacknowledged frames overlap
physics with disk recording. `progress.json` reports simulation/wall time
and physics, camera and recorder timings. The preview keeps the newest complete
frame so a slow or disconnected viewer cannot stall the source recording.
The navigation, guard and simulator nodes wait for the final recording
acknowledgment before exiting, so shutdown cannot cut off the last source frame.
It consumes a separate Dora stream of full-resolution JPEG previews (quality
90), rather than duplicating the large lossless sensor payload. This also keeps
brief preview-node suspensions below Dora's event-message size limit.

Studio presents all three RGB views, the articulated robot, moving base and
scene objects on one simulation clock. Signals and the independent replay window
follow the same sample. Live transport and curation are disabled. When the
source finishes, Live retains the final frame and offers **Open recording** to
open the finalized dataset in Replay for seeking, playback, tagging and
multi-window use. It remains ready for a new source session. Lost input changes
the status to waiting rather than pretending time is advancing.

Mobile scenes default to a close **View: Follow** camera; **View: Room** restores
the full room overview. Each window owns its own choice. Following updates only
the camera pivot, preserving manual orbit and zoom and leaving recorded poses
untouched. `verify_living_visuals.py RUN BASELINE` checks material/mesh provenance,
unchanged optical mounts, and task-object visibility in all three sensors using
their recorded segmentation masks.

Keep the entire run directory:

- `raw/frame_*.npz`: lossless RGB, metric depth, segmentation, camera and mount
  transforms for all three cameras; all 17 joints and position targets; proposed
  and applied wheel rates; base/object poses; every physics tick and contact pair.
- `scene-info.json`: optics, segmentation-to-entity/link map, physics body IDs,
  scene geometry and initial state. Segmentation is simulator ground truth.
- `raw/frames.jsonl`, `raw/control.parquet`: source hashes, clocks, node identities
  and command trace. `node-source/` contains the exact executed Python/graph code.
- `lerobot/`: 17-channel state/action Parquet, three **lossy RGB** MP4 copies,
  and version-2 scene replay including base poses. Studio displays RGB; depth
  and masks remain in the lossless archive. Version-1 fixed-base replay still works.

Verify every source frame, export row, camera attachment, segmentation ID,
physics contact classification, command bound, video frame and backward seek:

```sh
uv run --project ../AISLE --frozen --python 3.13.5 \
  --with 'opencv-python-headless==5.0.0.93' \
  python -B tools/aisle-validation/verify_living.py \
  tools/aisle-validation/runs/living-001
```

Acceptance requires all goals reached, more than 3 m of measured base travel,
actual cube contact and over 0.15 m cube displacement, and no robot/furniture
contact at any physics step. `verification.json` records the checks. Unit tests
are in `test_nano_living.py`; Rust tests cover synchronized preview frames,
malformed transport, new-session reconnects and base-pose replay validation.
Saved `DOROBOT_UX_REPORT` samples can additionally be checked against the source
with `verify_living.py RUN --ui-samples SAMPLES.json`: actual rendered base
matrices, 17 viewer joints, scene instances, signal values and live clocks are
compared, including an open independent replay window when present.

## What the fixed-base arm run does

The bundled [Nano model](../../data/adora_nano/README.md) supplies the actual link
geometry, joint axes, limits, and grippers. Each arm has five arm joints plus a
gripper. The complete state has **17 channels**: 12 arm/gripper channels, two head
joints, and three wheels. Joint names map the URDF order to Nexus's internal order;
the grippers are not contiguous with their respective arm chains.

The base is fixed, and the head and wheel joints receive zero position targets.
Three episodes exercise the left arm, right arm, and both arms. At the default
settings, each episode contains 30 samples: **90 samples total, 10 fps, three
640×480 cameras** (270 camera frames). Angles and targets are in radians.

These are joint-motion checks. Their `success` labels describe the checks below,
not successful grasping or a completed manipulation task.

## Nano's three onboard cameras

The default `--camera-rig nano` captures **head, left_wrist and right_wrist**.
Each is attached
to its URDF link (`head_tilt`, `left_wrist_cam`, `right_wrist_cam`). All three
render at the same simulation tick, at 640×480 and 10 Hz. Every scheduled frame
is retained: 90 frames per camera across the three default episodes, 270 camera
frames total. This is the configured simulation rate, not a hardware camera spec.

The URDF specifies camera housings but no optical frames or calibrated lenses.
`nano_cameras.py` therefore declares nominal mount offsets, optical axes and
70° vertical fields of view. These are simulation assumptions, not measured
Nano calibration. Replace them with measured calibration before a hardware
comparison.

Each raw NPZ contains lossless RGB, float32 metric depth, int64 segmentation,
simulation timestamp, the actual camera transform, and the actual mounting-link
transform. The manifest contains intrinsics, mount transforms, segmentation ID
mapping and capture policy. Segmentation is simulator ground truth. Keep the
whole run folder: `aisle-run/` is the complete raw archive; the `lerobot/` videos
are lossy RGB playback copies. Studio currently displays RGB, not depth/masks.

AISLE's existing offline exporter only joins the head stream (under its
`overhead` alias). The converter adds both wrist streams using exact same-tick
joins, rejects missing frames, and hashes every raw artifact in the provenance.
`verify_artifacts.py` checks all streams, hashes, frame counts, joint joins,
camera attachments and decoded video pixels. It separately reports the residual
between nominal URDF forward kinematics and the engine's actual link poses;
recorded transforms remain the actual render transforms.

Studio shows three distinct camera panes on one transport. Closing, maximizing,
dragging, resizing and opening a replay window work for the third pane too.
Existing four-panel layout preferences are migrated without discarding their
order or visibility. With three cameras the backend checker performs 54 video
seeks (six per camera per episode).

This addition does **not** turn the motion probe into a cube-pickup task. AISLE
at revision `845e31f5` has real grasp/lift/place experts for its registered Franka
and SO-101 embodiments; Nano is not registered. A Nano cube task still needs
its own grasp controller/IK, a free cube and contact dynamics, phase and object
state recording, and lift/placement outcome checks. A moving arm alone is not
evidence of a successful pickup. See AISLE's
[Nexus coverage](https://github.com/moxin-org/AISLE/blob/845e31f5d1871905e69c9d838f90a32c62bd6476/docs/getting-started.md).

## Foreground workspace and UI inspection

For the foreground workspace recording, add `--workspace --frames 100
--camera-rig nano` to the collection command. This records three 10-second
episodes with a larger, smooth multi-stage arm/gripper sweep, six colored free
rigid-body cubes, and a fixed workbench. The cubes fall and settle under Nexus
physics. No object is attached to a gripper and no pickup success is claimed.

`nano_scene__objects.arrow` retains the actual object transforms at each camera
tick. `lerobot/meta/scene/episode_NNNNNN.json` projects those exact transforms
onto the same playback rows. Studio renders the seven objects alongside the
URDF; it validates the sidecar's geometry, frame count and timestamps before
loading it. The artifact checker compares every sidecar pose to the Arrow
source. Recordings without this optional sidecar keep their existing behavior.

For UI inspection without desktop interaction, this pinned Makepad runtime
supports `MAKEPAD_HAL_BACKEND=headless` (layout/events, no raster) and
`MAKEPAD_HAL_BACKEND=offscreen` (Metal rendering without OS windows). Its
`makepad-remote app <pid> count <function>`, `probes` and `draws` commands provide
runtime instrumentation; there is no separate `instrument` subcommand at this
pin. Set `DOROBOT_UX_REPORT` to enable read-only view telemetry. The report now
includes actual viewer joint angles, TCP link transforms and scene instances,
so checking only that the playhead advanced cannot pass as URDF-motion evidence.

The workspace check compares source states, named URDF poses, independent
forward kinematics and actual scene instances at frames 0, 30, 60, 99 and 20 in
each episode, including backward seeks. Offscreen screenshots additionally
show the changed robot geometry. Headless results are not pixel evidence.

## Prerequisites and versions

- macOS on Apple Silicon for the tested **Metal** path.
- `uv` and Python 3.13.5, a current stable Rust toolchain, and Git.
- FFmpeg development libraries and `pkg-config` for Studio's real video decoder.
  On macOS: `brew install ffmpeg pkg-config`.
- A Studio checkout containing this directory and `data/adora_nano/`.
- An [AISLE checkout](https://github.com/moxin-org/AISLE), preferably alongside
  Studio. If needed, from the Studio root: `git clone https://github.com/moxin-org/AISLE.git ../AISLE`.

The original reference run used AISLE commit
`845e31f5d1871905e69c9d838f90a32c62bd6476`, Studio commit
`f3365cf26066fb3fecbfe7495cfb3e8a3478003a`, `dimforge-nexus3d==0.2.1`, and
`opencv-python-headless==5.0.0.93`. The commands below pin those two overlay
packages and use AISLE's lockfile for its base dependencies. For exact source
reproduction, use that AISLE commit in a dedicated clone. This probe imports
internal AISLE helpers, so later AISLE changes may require an update.

The packaged workflow was rechecked on 2026-11-14 against Studio
`feabe3758f07bd9ddfd60f93e03666f6f05ff5e3`: all six model tests, three Nexus
episodes, 90 source-to-dataset rows, and Studio backend checks passed.

The Python commands do not require AISLE's full `sim` extra or a separate Nexus
source build. `uv` supplies the pinned Dora API and Nexus wheels in an overlay
environment. Install the matching Dora CLI once:

```sh
uv tool install --with 'dora-rs==1.0.1' 'dora-rs-cli==1.0.1'
```

Use `--dora /path/to/dora` if the executable is not on `PATH`. The collector
checks CLI/API versions before launching. This is development evidence, not an
attested AISLE benchmark environment.

## 1. Set paths and check the Nano contract

Start in the **Studio repository root**, not in this directory:

```sh
studio_root="$PWD"
aisle_root="$(cd ../AISLE && pwd)"  # change this if AISLE is elsewhere
run_root="$studio_root/tools/aisle-validation/runs/nano-bimanual-001"

uv run --project "$aisle_root" --frozen --python 3.13.5 \
  python -B -m unittest discover -s tools/aisle-validation \
  -p 'test_nano*.py' -v
```

These tests check model mapping, trajectories, command admission and recording
barriers without launching simulation. To also send NaN, stale, out-of-range
and too-fast commands through the real Dora guard, run:

```sh
NANO_DORA_GRAPH_TESTS=1 uv run --project "$aisle_root" --frozen --python 3.13.5 \
  --with 'dora-rs==1.0.1' python -B -m unittest discover \
  -s tools/aisle-validation -p 'test_nano*.py' -v
```

Use a **new `run_root` for each run**. The collector refuses an existing output
directory. Generated runs are ignored by Git.

## 2. Collect motion in Nexus and export through AISLE

```sh
uv run --project "$aisle_root" --frozen --python 3.13.5 \
  --with 'dora-rs==1.0.1' \
  --with 'dimforge-nexus3d==0.2.1' \
  --with 'opencv-python-headless==5.0.0.93' \
  python -B tools/aisle-validation/nano_validation.py \
  --aisle "$aisle_root" \
  --model "$studio_root/data/adora_nano/adora_nano.urdf" \
  --backend metal --output "$run_root"
```

This launches the Dora graph, runs actual Nexus physics steps, records Arrow
traces and raw RGB/depth/segmentation captures, invokes `tools/demo_dataset.py`,
and creates the Studio dataset. Add `--workspace --frames 100` for three
10-second clips with six foreground cubes. The checkers support either length.
The previous in-process collector remains available only as the explicit
`--transport direct` reference path; use `--camera-rig observer` with that path
to reproduce the original single-camera fixture.

Verify the actual Dora graph and received payloads before checking the export:

```sh
uv run --project "$aisle_root" --frozen --python 3.13.5 \
  python -B tools/aisle-validation/verify_dora.py "$run_root"
```

`dora-verification.json` checks distinct processes, routing, every turn barrier,
guard-to-applied command equality, sensor synchronization, source hashes and
the archived camera bytes against the payload hashes received through Dora.

The collector writes `result.json` and exits successfully only when all motion
checks pass. The per-episode checks require finite states, maximum tracking error
below 0.08 rad, joint-limit excess below 0.03 rad, and measured excursion above
0.03 rad for each commanded arm. Failed episode outcomes remain in the evidence.
The exporter refuses ineligible samples instead of silently dropping them.

`--backend webgpu` and `--backend cpu` are exposed for Nexus development, but
this guide's validation results cover Metal only.

## 3. Check the dataset against the original traces

```sh
uv run --project "$aisle_root" --frozen --python 3.13.5 \
  --with 'opencv-python-headless==5.0.0.93' \
  python -B tools/aisle-validation/verify_artifacts.py "$run_root"
```

This independently reads the original Arrow streams and the converted Parquet
and MP4 files. It checks every row for joint names, state/action values, episode
and frame indexes, timestamps, and video frame counts. It also compares decoded
video pixels with the original RGB captures, allowing for lossy video encoding.
The result is `artifact-verification.json`.

AISLE joins observations strictly before a command in simulation time. Parquet
timestamps are episode-relative command times; the paired camera and state
timestamps remain separately recorded in `lerobot/meta/aisle_provenance.json`.
Neither the converter nor the checker invents a future observation or silently
resamples an irregular recording.

## 4. Exercise Studio's production backend

```sh
uv run --project "$aisle_root" --frozen --python 3.13.5 \
  python -B tools/aisle-validation/check_studio.py \
  "$run_root/lerobot" --studio "$studio_root"
```

Use the freshly generated fixture: this step deliberately changes its curation
sidecars while testing tags and exclusion. It verifies:

| Function | Assertion |
| --- | --- |
| Library and episode loading | One dataset, three episodes, all recorded frames, correct Nano model |
| Joint playback | All 17 state/action channels load; radians reach the URDF pose stream unchanged |
| Video playback | 54 seeks for the three-camera fixture, including backward seeks and final frames, return correctly sized RGB frames |
| Transport | Seek, frame-step, and play/pause intents update backend state correctly |
| Curation | A tag survives reopening; exclusion/Undo preserves the original Parquet bytes |

The script temporarily installs `check_nano_aisle.rs` as a Cargo example in
`dorobot-flex/examples/`. It refuses to overwrite an existing example and removes
its temporary example and build directory after the run. It writes
`studio-verification.json` and `studio-integration.log` beside the dataset.

These checks call the same production backend used by the UI. They do not click
the desktop window, validate its visual layout, or exercise the Record/Eval
backends.

## 5. Open the recording in Studio

Run from the Studio root so the bundled Nano URDF resolves:

```sh
cargo run --locked -p dorobot-flex -- \
  --dataset "$run_root/lerobot" --screen play
```

Keep default features enabled for video playback. For the current JIT launcher,
the corresponding command is:

```sh
./tools/run_jit.sh --video --dataset "$run_root/lerobot" --screen play
```

Select each episode and inspect both arms, the three camera videos, and the
timeline. The default Dora fixture has head, left-wrist and right-wrist camera
panes synchronized with the 3D Nano view.
The reader, robot pose stream, and state/action plots retain all 17 channels. The Signals panel pages
through them, puts moving joints first, and shows the playback cursor and sampled
Actual/Target values. The checker validates every plotted channel against the
recorded samples.

## Artifacts and interpretation

```text
<run_root>/
├── aisle-run/                     Original traces, RGB arrays, model contract, outcomes
├── aisle-export/                  AISLE's verified offline demonstration export
├── lerobot/
│   ├── meta/info.json             17-channel names, fps, dimensions and paths
│   ├── meta/episodes.jsonl        Episode index
│   ├── meta/tasks.jsonl           Motion-check descriptions
│   ├── meta/aisle_provenance.json Source timestamps, hashes, mappings and outcomes
│   ├── data/chunk-000/            Per-episode Parquet
│   └── videos/chunk-000/          Head and both wrist camera MP4s
├── dataflow.yaml                 Resolved Dora graph used by this run
├── node-source/                  Immutable snapshot of executed Python nodes
├── dora-verification.json        Dora processes, turns, routing and payload checks
├── result.json                    Simulation checks and AISLE export coverage
├── artifact-verification.json     Source-to-dataset checks
├── studio-verification.json       Production Studio backend checks
└── studio-integration.log         Cargo build and decoder diagnostics
```

Check `ok: true` in the result, Dora, artifact and Studio reports. A passing
simulation report alone does not establish that Studio imported the dataset correctly.

The original Metal reference run passed all three episodes and all six contract
tests. AISLE exported all 90 commands as eligible samples, with no unassigned
commands or exclusions. Maximum tracking error was approximately **0.0178 rad**;
no measured joint-limit excess occurred. Studio preserved all 17 pose channels
and passed its 18 video seeks, transport, and curation checks. These are recorded
software-test results, not tolerances guaranteed on every engine or machine.

The Dora migration was also run with `--workspace --frames 100`: five separate
Dora processes completed 304 acknowledged turns and 3,003 physics steps.
All 300 commands matched their guarded inputs, and all 900 received camera
payloads matched the saved bytes. AISLE exported every sample without exclusion.
The three motion checks passed (maximum tracking error 0.04852 rad; no measured
joint-limit excess). Studio passed 54 video seek checks and nine UI samples
across all three episodes, including all 17 joints, rendered link transforms
and seven scene objects. Twelve tests passed, including real Dora rejection of
NaN, stale, out-of-range and excessive-velocity commands. This remains a motion
and dataflow check; it does not add a Nano grasp policy.

## Scope and troubleshooting

- The model uses nominal masses/inertias and AISLE's default URDF gains and
  damping. This run does not establish contact fidelity, grasping, bimanual task
  competence, mobile-base control, hardware calibration, or robot safety.
- In Dora mode, `nano_limits/joint_cmd_safe` is a real guarded graph output.
  AISLE's exported `nano_sim__joint_cmd_safe.arrow` records the matching bridge
  application confirmation at its actual physics timestamp; raw bus timestamps
  and payload hashes remain in `aisle-run/dora-events.jsonl`. The external Nano
  guard is separate from AISLE's benchmark budget-guard implementation.
- This is a Studio regression fixture, not a certified training dataset. The
  `training_samples` coverage field is AISLE's eligibility count, not a training
  quality verdict.
- Physical hardware, Studio recording, policy Eval, Hub
  transfers, and desktop mouse/keyboard acceptance are outside these checks.
- If the collector reports an existing output directory, choose a new run name.
- If a Nexus API is missing, compare your AISLE revision and Nexus wheel with the
  versions above. Do not replace the physics run with a synthetic preview.
- If the Studio build cannot find FFmpeg, check `pkg-config --modversion libavcodec`
  and the [Studio setup guide](../../docs/FUNCTIONAL_REVIEW.md). A build without
  video support cannot validate this recording's video path.
- If the robot view is empty, launch from the Studio root and check that
  `data/adora_nano/adora_nano.urdf` and its relative mesh paths exist.
- If a Studio check fails, inspect `studio-integration.log`; the recording and
  source evidence remain available for diagnosis.
