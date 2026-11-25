# Repeat a Nano or G1 experiment in Nexus

Prepare a controller for a saved scene in **Task**. Open **Run → Repeat experiment**. The heading shows the robot, instruction, pinned scene revision, controller and selected worker. Configurations created before worker selection was available open with the local Nexus worker.

1. Name the configuration and enter the number of repetitions. Choose sequential seeds, an explicit list, or identical inputs. A sequential run starts at the base seed. An explicit list needs one unsigned 32-bit seed for each repetition.
2. Choose **Batch defaults** to set **Robot trace** and **Agent history** independently. Choose a repetition to override either switch. **Use batch defaults** clears that repetition’s overrides. **Record cameras** controls the replay dataset. Nano continues to use its onboard RGB and encoders in all four log modes.
3. To vary chair spacing, select **Pair spacing** and the two chair instances. Choose X or Y. Set a numeric feature name, units of `m`, default, minimum and maximum. Choose **Apply feature**. The sampler moves both chairs around their original midpoint. Position, yaw, friction, ambient light, wrist clearance and camera-profile bindings are also available.
4. Choose **Preview layout** before starting the batch. **Previous layout** and **Next layout** show each repetition's top view, seed, realized values and layout digest prefix. An invalid sample shows its geometry error. Adjust the feature, choose **Apply feature** and preview again. Preview uses the same sampler and geometry checks as batch preparation. The seed 43 preview shows 2.7647 m between the chairs. [Native preview](experiment-verification/sampled-layout.png)
5. **Save configuration** stores the settings. **Start batch** freezes the controller, bound task, scene and assets. The supervisor runs independently of the window. Use **Previous trial** and **Next trial** to inspect the realized seed, parameters and effective recording policy. Cancel a pending or running trial. A retry receives a fresh run ID and retains its parent ID.
6. Reopen **Run → Repeat experiment** to inspect the batch. The project stores the batch under `aisle/experiments/<batch-id>`. Each `trials/<run-id>/` contains the manifest and sampled layout. A finished physics run includes its measured result. Enabled logs appear as `robot-trace.jsonl` and `agent-history.jsonl`.

Robot traces contain measured and commanded joint values, physics steps and contact or guard information. Agent history identifies which provider prepared the controller and when a run reused it. The reference controller yields an explicit reference event. Camera, trace and agent history can therefore have different availability for one trial.

## Run on a registered worker

1. Set `DOROBOT_AISLE_WORKERS` on the Studio machine to the path of a worker registry. Each alias has a Python interpreter, Studio checkout, AISLE checkout and initialized worker spool. For SSH transport, set `ssh` to an SSH configuration alias. These paths refer to the worker. Initialize a new spool on that machine with `"$DOROBOT_AISLE_PYTHON" tools/aisle_workbench/fleet_worker.py --root "$WORKER_SPOOL" init --slots 2`. Choose the capacity appropriate for that worker.
2. Select the alias under **Run configuration → Worker**. Studio checks the batch protocol and the digest of the selected robot model. The panel displays the Nexus backend, engine version and occupied slots. **Check worker** refreshes those values. **Start batch** becomes available for a compatible worker. A saved configuration pins the worker’s identity, so replacing the machine behind an alias requires choosing it again.
3. Preview a seed and start the batch. The remote worker independently samples the frozen configuration and checks the layout digest before it acquires a simulation slot. The batch waits if all slots are busy. The camera, robot trace and agent history choices travel with the repetition. When history is enabled, the worker receives the redacted events retained by Studio.
4. Reopen Run after closing Studio. A running repetition keeps the same lease ID. Studio retrieves its status and verifies the completed archive checksum before attaching its files to the trial in Evidence. **Cancel trial** targets the selected lease. **Retry trial** allocates a new ID and retains the original seed and parent ID.

A registry can contain an entry like this. Use absolute paths on your worker and an SSH alias already available to the account running Studio.

```json
{
  "lab-mini": {
    "ssh": "lab-mini",
    "python": "/opt/aisle/venv/bin/python",
    "checkout": "/opt/aisle/studio",
    "aisle_root": "/opt/aisle/runtime",
    "spool": "/opt/aisle/experiment-spool"
  }
}
```

In the offscreen G1 check, Studio rejected the incompatible worker, displayed the compatible worker’s Metal backend and slot occupancy, then recovered the same lease after the app and its local batch supervisor exited. A separate trial cancelled an active remote G1 lease and retried its seed with camera recording enabled. The retry completed 2,000 physics steps and returned 100 operator-camera frames with the optional trace and agent history disabled. [Worker selection](experiment-verification/registered-worker/worker-selection.png) · [Offscreen receipt](experiment-verification/registered-worker/g1-offscreen.json) · [Cancellation, retry and camera measurements](experiment-verification/registered-worker/cancel-and-retry.json) The G1 headless journey passed again after Studio added launch checks for saved controller and scene revisions and sampled camera capabilities. [Final preflight receipt](experiment-verification/registered-worker/g1-preflight.json)

Both provider histories were exercised on the mini. Studio sent previously prepared Codex and DeepSeek via Octos standing-reach controllers. Each returned the original prompt and response linked to the new run, plus a controller-reuse event. Both runs held foot contact for 2,000 of 2,000 steps. The worker made zero new model calls. These trials measure execution and history portability. [Codex receipt](experiment-verification/registered-worker/codex-history.json) · [DeepSeek via Octos receipt](experiment-verification/registered-worker/octos-deepseek-history.json)

## Limit an object's starting position

1. Open **Scene → Scene editor**, select the object, and scroll to **Randomized start area**. Choose the floor or a named asset surface. Enter X and Y limits in metres relative to the support centre, or leave all four fields empty to use the full support. When the support rotates, its local X and Y axes rotate with it.
2. Choose **Set start area**. The viewport outlines the permitted region. Studio checks room or surface edges and the object's rotated collider footprint. Save the scene revision and bind it in Task. The original pose remains the starting pose until the rule is enabled for a run. [Native area editor](experiment-verification/tabletop-area.png)
3. In **Run → Repeat experiment → Scene start areas**, select each object to vary and choose **Enable area**. Open **Preview layout** to inspect the outlines and sampled world coordinates. Start the batch to retain each object's sampled local and world positions with that repetition. The saved configuration restores the rule selections when Studio reopens. [G1 room preview](experiment-verification/area-preview.png)

The two native checks authored a floor region for the red cube and a named tabletop region for the blue cube. They rejected an out-of-bounds region, retained the original scene revision and ran G1 for 2,000 steps on each of two seeds. In the 20° rotated tabletop frame the blue cube settled at (0.2846, −0.0623) m and (0.2182, −0.1359) m. Both backends reproduced the same layout digests. The measured cube bottoms were within 0.002 mm of their support planes in Nexus. These measurements describe simulator geometry. [Offscreen measurements](experiment-verification/start-areas-offscreen.json) · [Headless measurements](experiment-verification/start-areas-headless.json)

## Reuse lighting, material and pose ranges from a scene

In **Scene editor → Experiment ranges**, choose **New range**. Name the range, select a property and enter its minimum and maximum. World X, Y and Z use metres, yaw uses degrees and surface friction is dimensionless. Choose the target object for these properties. Room ambient light accepts a scalar from zero to one for all three color channels. Select **Save range**, then **Save revision**. Reopen a range to edit or remove it.

Bind the new scene revision on **Task** and prepare a controller. In **Run → Repeat experiment → Scene experiment ranges**, select and enable each range you want. **Preview layout** shows the sampled values and layout digest for each seed. **Save configuration** retains the enabled combination. Each trial records its realized values, originating scene rule and layout digest.

In the offscreen G1 run, seeds 42 and 43 placed the left chair at −1.574 m and −1.587 m on the X axis, with measured yaw of +6.965° and −2.509°. Nexus used ambient RGB levels of 0.374 and 0.261. The saved collider configuration applied friction coefficients of 0.756 and 0.321 to all five table colliders. Each G1 trial completed 2,000 steps. [Native Scene editor](experiment-verification/scene-ranges.png) · [Run preview](experiment-verification/scene-range-preview.png) · [Offscreen receipt](experiment-verification/scene-ranges-offscreen.json)

Nano also exercised the same four ranges. In the dimmer scene, its original controller lifted the red cube and timed out while looking for the table rim. The recorded exposure and frozen pair are available for replay. The revised controller confirmed the rim in its head and wrist images and placed the cube with 100 consecutive ticks of table contact. [Reproduce the failure and correction](experiment-verification/nano-dim-oak/README.md)

## Measured runs

Each native test selected the two chairs and configured five repetitions. The first four placed the selected red cube on the named tabletop. The operator cancelled the fifth before execution and reopened Studio during the batch. Every recorded robot row was checked for its simulation timestamp, 17 joint values, commands and wheel support. Each retained history was checked against the frozen controller and run ID.

| Seed | Trace | Agent history | Measured chair spacing |
| --- | --- | --- | --- |
| 42 | Off | Off | 1.9317 m |
| 43 | On | Off | 2.7647 m |
| 44 | Off | On | 2.4638 m |
| 45 | On | On | 2.2103 m |

The offscreen DeepSeek/Octos controller ran 11,266 physics steps per completed repetition. The headless reference controller ran 6,848 per repetition on a second Mac. The verifier reproduced every layout from its seed and compared the sampled spacing with the two instantiated Nexus bodies. These runs exercised a local worker on each machine. [Offscreen receipt](experiment-verification/octos-offscreen.json) · [Headless receipt](experiment-verification/reference-headless.json) · [Native batch results](experiment-verification/batch-results.png)

## Reproduce

Use the pinned AISLE runtime, Python environment and Makepad remote tool from the [setup guide](../../../../README.md). Give each test a fresh session directory.

```sh
python3 tools/check_experiment.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-repeat-headless --backend headless
python3 tools/check_experiment.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-repeat-octos --backend offscreen --provider octos
python3 tools/check_start_areas.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/g1-start-areas --backend headless
python3 tools/check_scene_ranges.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-scene-ranges --robot Nano --backend headless
```

To include the archived comparison tests, set `DOROBOT_VALIDATION_ARCHIVES` to the `docs/product/aisle-studio/validation` directory in the companion Dorobot Studio checkout. To exercise the Dora guard graph, set `NANO_DORA_GRAPH_TESTS=1` and point `DORA_BIN` to the Dora 1.0.1 executable. Run `"$DOROBOT_AISLE_PYTHON" -m unittest discover -s tools/aisle-validation` from the AISLE Studio root. [Companion validation archives](https://github.com/dorobot-org/dorobot-studio/tree/9bc37162c7548287659fbed48acb79648d90bd9d/docs/product/aisle-studio/validation)
