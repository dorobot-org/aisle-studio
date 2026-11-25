# Nano three-cube qualification, second release

The frozen controller completed every transfer on **39/50** fresh registered layouts. The release requirement was 45 complete layouts, all 50 verified and zero guard bypasses. The independent replay verified 50 distinct skill executions and counted 0 guard bypasses. The registered gate was **missed**. [Registration](registration.json) · [Verdict](verification.json) · [Raw evidence](evidence.zip) · [Archive digest](archive-sha256.txt)

The 95% Wilson interval for three-cube success was 64.8%–87.2%. The red cube met the contact and settling criteria in 50 layouts, green in 47 and blue in 39. These counts include partially completed tasks.

The evaluator measured 1,308,128 wheel-supported ticks across 1,308,128 simulation ticks, 0 rejected motor writes, and robot–furniture contact in 0 layouts. The maximum measured excursion beyond a joint limit was 0.00000 rad. [Machine-readable totals](outcomes.json)

## Failed layouts

| Registered case | Cubes meeting all criteria | Measured reason |
| --- | --- | --- |
| 001 | red, green | No reachable clear release point on the visible tabletop. Blue recorded 3714 airborne grip ticks and 0 final tabletop contact ticks. |
| 003 | red, green | Cannot track an earlier cube through a placement turn. Blue recorded 3092 airborne grip ticks and 0 final tabletop contact ticks. |
| 007 | red | Lost the observed cubes across consecutive placement-turn images. Green recorded 3807 airborne grip ticks and 0 final tabletop contact ticks. Blue recorded 0 airborne grip ticks and 0 final tabletop contact ticks. |
| 011 | red, green | No reachable clear release point on the visible tabletop. Blue recorded 4006 airborne grip ticks and 0 final tabletop contact ticks. |
| 026 | red, green | Cannot track an earlier cube through a placement turn. Blue recorded 3170 airborne grip ticks and 0 final tabletop contact ticks. |
| 028 | red, green | Cube motion across the placement turn is inconsistent. Blue recorded 3359 airborne grip ticks and 0 final tabletop contact ticks. |
| 035 | red, green | Lost the observed cubes across consecutive placement-turn images. Blue recorded 3963 airborne grip ticks and 0 final tabletop contact ticks. |
| 041 | red | Lost the observed cubes across consecutive placement-turn images. Green recorded 4724 airborne grip ticks and 0 final tabletop contact ticks. Blue recorded 0 airborne grip ticks and 0 final tabletop contact ticks. |
| 043 | red | Lost the observed cubes across consecutive placement-turn images. Green recorded 3306 airborne grip ticks and 0 final tabletop contact ticks. Blue recorded 0 airborne grip ticks and 0 final tabletop contact ticks. |
| 044 | red, green | Lost the observed cubes across consecutive placement-turn images. Blue recorded 3488 airborne grip ticks and 0 final tabletop contact ticks. |
| 048 | red, green | Lost the observed cubes across consecutive placement-turn images. Blue recorded 3244 airborne grip ticks and 0 final tabletop contact ticks. |

The original release scored 44/50 on a different set of registered layouts. Only the 12 disclosed development cases were run as a paired controller comparison. In those cases the selected revision finished 11/12 and preserved every previously passing control. The raw release archive was restored into a separate project and its motor journals and bridge receipts reproduced the published verdict. [Original release](../outcomes.md) · [Paired development comparison](../wrist-survey/README.md)

The signed Makepad DevHub worker reverified the full cohort from its raw archive. The native review showed the below-threshold result, prevented approval and signed export, and rejected a changed measurement. A second lease opened the original camera samples. [Native review](../../../devhub/nano-release-2-review/nano/verification.json) · [Native case inspection and images](../../case-inspector/README.md#inspect-a-released-cohort)
