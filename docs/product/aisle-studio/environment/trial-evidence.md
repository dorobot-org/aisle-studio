# Replay and revise an experiment trial

Open **Evidence → Experiment trials**. Search by run ID, name, scene, instruction, robot, tag or outcome. Each repetition has its own entry. Select a completed trial to see its measured outcome, robot, engine, seed and scene revision.

## Review the recording

Select **Head**, **Left wrist** or **Right wrist**, then play, pause, drag the timeline, advance one frame, or jump to a controller phase such as *place*. Studio uses the physics step stored with each RGB frame to find the corresponding robot state and commands. The displayed phase is the most recent marker at that step. Agent preparation has its own wall-clock timestamps. The history selector shows the saved prompt, response and available tool records with the provider, model and stage.

The camera dataset, agent history and robot trace have independent availability. If the camera dataset is off and robot tracing is on, the timeline provides trace samples from the actual recording. With only agent history enabled, the history inspector remains available. The verdict and execution identity stay available in all four optional-log combinations. [Native camera-off checks](evidence-verification/camera-off.json)

## Edit notes or change the experiment

- Enter a run name, notes, comma-separated tags and custom metadata as a JSON object. **Save notes** creates a numbered annotation revision. Select a previous revision to inspect it. A stale edit prompts a refresh. These records live alongside the batch in `annotations/<run-id>/`. Raw trial artifacts retain their original bytes.
- **Rerun original** creates a new run ID from the parent’s frozen task, controller, seed, effective features and recording policy.
- For a variant, enter a name, select the object and coordinate, and edit its position in metres. The coordinate is relative to the saved room. Enter a comma-separated list of seeds for multiple repetitions. **Variant options** also exposes the recording policy and typed runtime features. To use a revised scene, first save the scene and prepare its bound task on **Task**, then choose **Prepared selection from Task** in the variant options. Studio verifies and freezes that controller and its companion files.
- Choose **Save As**, review the changes and choose **Run edited version**. The child’s inspector shows its parent ID, both measured outcomes, contact steps and changed positions. **Open parent** takes you back to the source. Each new execution has a distinct ID even if its sampled inputs match exactly.

## What was measured

The native acceptance test opened a recorded Nano pickup, changed camera, played and paused, dragged the timeline, and jumped to placement. It renamed the run, added searchable metadata, moved the left chair along Y, restarted Studio, then launched both the edited and original configurations through Evidence. It inspected the independent Nexus body measurements after both executions. Finally it rehashed every original trial file, including all camera frames.

| Native backend | Frames per camera | Placement frame physics step | Original files rehashed | Completed physics steps per run |
| --- | ---: | ---: | ---: | ---: |
| Offscreen, DeepSeek via Octos | 563 | 9,020 | 1,765 | 11,266 |
| Headless, reference controller | 342 | 4,620 | 1,088 | 6,848 |

On both machines, the original measured chair Y was −1.700 m, the variant measured −2.000 m, and the original rerun reproduced −1.700 m and the parent’s layout digest. All six executions passed the selected-cube placement criteria. [Offscreen receipt](evidence-verification/octos-offscreen.json) · [Headless receipt](evidence-verification/reference-headless.json) · [Wrist playback](evidence-verification/recorded-wrist.png) · [Variant review](evidence-verification/variant-changes.png) · [Parent comparison](evidence-verification/parent-comparison.png)

## Reproduce

Start with the [four-repeat example](experiments.md#reproduce). Choose the batch directory that contains its `batch.json`. The check makes an isolated project. If the source has no camera recording, the check first runs a new camera-enabled baseline with its pinned controller. Supply the same AISLE source and Python environment used for the batch.

```sh
python3 tools/check_trial_evidence.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-evidence \
  --source-batch "$BATCH" --backend offscreen
python3 tools/check_trial_evidence.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-channel-views \
  --source-batch "$BATCH" --backend headless --channels-only
```
