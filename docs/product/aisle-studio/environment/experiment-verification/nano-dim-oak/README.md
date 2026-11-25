# Reproduce Nano’s low-light docking trial

On seed 43, Nano picked up the red cube and timed out while looking for the coffee table. The original DevHub trial and a replay with all three cameras recording produced identical motor targets, action logs and cube measurements. DevHub job J7 was cancelled after both physics trials reached a verdict. The original seed 42 succeeded.

The two images below come from the same exposure at physics tick 4,780 of the failed replay. In each image the 10th, 50th and 90th percentiles of brightness among oak-colored pixels were all 78 on a scale of 0–255. The controller required at least 88. Its new version retries in dim light and requires sufficient color contrast, a horizontal edge consistent across cameras, positive depth and overlapping visible spans. Camera poses come from the URDF and measured joint angles. The two views yielded a table height of 0.541 m and a distance of 0.483 m from the base. [Head RGB](head.png) · [Right wrist RGB](right_wrist.png) · [Camera matrices and measurements](comparison.json)

| Seed 43 | Original controller | Revised controller |
| --- | ---: | ---: |
| Termination | Docking timeout | Cube settled |
| Physics ticks | 6,678 | 6,908 |
| Ticks held airborne by opposing fingers | 2,294 | 3,210 |
| Final 100 ticks with measured table contact | 0 | 100 |
| Cube centre inside the tabletop edge at rest | No | 87.3 mm |
| Invalid motor rows | 0 | 0 |

The candidate registered 40 confirmed dim-table observations. Both versions used the same task, URDF, seed, sampled scene and physics scoring. New Studio controllers for an authored oak-table transfer record the perception version in their controller file. Older controller files replay their original thresholds. These results cover a single red-cube transfer in simulation.

DevHub J8 exercised both seeds again through Makepad’s native Run page. Seed 42 generated the same motor targets as the original and settled 93.7 mm inside the table edge. The repaired seed 43 matched the motor targets and per-cube measurements of the direct replay. [Headless Nano measurements](../../devhub/scene-ranges/nano-ranges/verification.json)

## Run the frozen pair

From the AISLE Studio repository root, use the configured AISLE Python interpreter and two fresh output directories. The first trial’s expected result is `task_success=false`. Read both `result.json` files for physics verdicts and both `actions.jsonl` files for observations. [Runtime setup](../../../../../../README.md#run-locally)

```sh
fixture=docs/product/aisle-studio/environment/experiment-verification/nano-dim-oak
for version in baseline candidate; do
  "$DOROBOT_AISLE_PYTHON" tools/aisle_workbench/worker.py run \
    --robot Nano --seed 43 --controller "$fixture/$version.json" \
    --experiment-run "$fixture/$version-request.json" \
    --out "/tmp/nano-dim-oak-$version"
done
```
