# Recheck the tabletop after a placement turn

The second Nano release failed eleven of its fifty registered layouts. Nine failures involved tracking previously placed cubes during a turn. Two found no reachable clear slot. We ran both proposed revisions on all eleven failures and six previously passing controls. The controls included three successful layouts that already needed a placement turn and three successful layouts without that turn. [Original results](../release-2/outcomes.md)

Both revisions keep the original stationary grasp, docking distance, collision clearance and joint limits. When the regular placement procedure cannot find a clear slot or loses previous cubes, Nano brakes. Its calibrated onboard RGB cameras measure the table rim during a bounded turn. Once stopped, the spare wrist checks the earlier colors from above. A release after recovery requires a fresh observation of every earlier color, confirmation of the carried cube, and reachable inbound and withdrawal paths.

| Controller | Complete layouts | Previous failures recovered | Passing controls preserved |
| --- | ---: | ---: | ---: |
| Second release | 6/17 | 0/11 | 6/6 |
| Stop and resurvey, original release grid | 15/17 | 9/11 | 6/6 |
| Stop and resurvey, extended release grid | 17/17 | 11/11 | 6/6 |

We selected **Stop and resurvey, extended release grid**. On the earlier twelve disclosed layouts it completed 11/12 and preserved every previous success. Layout 43 again timed out while docking, before placement recovery could run. The matched runs used the frozen DeepSeek/Octos task plan with revised local perception and motion code. [Every result, controller identity and bridge receipt](development.json)

## A measured release on layout 1

On the blue release, the spare wrist located both earlier cubes. The extended grid found an arm path 6.5 cm inside the observed rim. The independent evaluator measured the blue cube’s settled center 9.35 cm from the nearest physical edge. It recorded 100 final table-contact ticks and wheel support throughout all 25,324 ticks. [Wrist image](case-001-right-wrist.png) · [Observation, selected point and independent measurement](case-001-geometry.json)

The development cases were already disclosed. The fresh, preregistered 50-layout run supplies the simulation release decision. The exact screened controllers and policy patches are provided beside the development record. Apply a patch to the release checkout with `git apply --unidiff-zero` to reproduce the corresponding policy bytes.

Across the selected controller’s 17 second-release development layouts, the evaluator recorded wheel support for 472,448 of 472,448 ticks, 0 rejected motor writes and robot–furniture contact on 0 layouts. The maximum joint-limit excursion was 0.00000 rad. [Physics totals for all comparisons](development-physics.json)

## Review the retry in Studio

The native Makepad Evidence page retried the same failed layout with the exact frozen controller. All three cubes passed with 0 guard bypasses. The operator reopened the project and switched between cases. Studio restored the correct trial and its three synchronized cameras. Across two displayed exposures, the largest mean absolute error for any camera against its recorded RGB pixels was 1.15 on a 0–255 scale. [Studio screenshot](native-case-001.png) · [Headless replay receipt](native-case-001.json)
