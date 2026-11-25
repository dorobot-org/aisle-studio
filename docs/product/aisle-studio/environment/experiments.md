# Repeat a Nano experiment in Nexus

Prepare a controller for a saved scene in **Task**. Open **Run → Repeat experiment**. The heading shows the robot, instruction, pinned scene revision, controller and local Nexus worker.

1. Name the configuration and enter the number of repetitions. Choose sequential seeds, an explicit list, or identical inputs. A sequential run starts at the base seed. An explicit list needs one unsigned 32-bit seed for each repetition.
2. Choose **Batch defaults** to set **Robot trace** and **Agent history** independently. Choose a repetition to override either switch. **Use batch defaults** clears that repetition’s overrides. **Record cameras** controls the replay dataset. Nano continues to use its onboard RGB and encoders in all four log modes.
3. To vary chair spacing, select **Pair spacing** and the two chair instances. Choose X or Y. Set a numeric feature name, units of `m`, default, minimum and maximum. Choose **Apply feature**. The sampler moves both chairs around their original midpoint. Position, yaw, friction, ambient light, wrist clearance and camera-profile bindings are also available.
4. **Save configuration** stores the settings. **Start batch** freezes the controller, bound task, scene and assets. The supervisor runs independently of the window. Use **Previous trial** and **Next trial** to inspect the realized seed, parameters and effective recording policy. Cancel a pending or running trial. A retry receives a fresh run ID and retains its parent ID.
5. Reopen **Run → Repeat experiment** to inspect the batch. The project stores the batch under `aisle/experiments/<batch-id>`. Each `trials/<run-id>/` contains the manifest and sampled layout. A finished physics run includes its measured result. Enabled logs appear as `robot-trace.jsonl` and `agent-history.jsonl`.

Robot traces contain measured and commanded joint values, physics steps and contact or guard information. Agent history identifies which provider prepared the controller and when a run reused it. The reference controller yields an explicit reference event. Camera, trace and agent history can therefore have different availability for one trial.

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
```
