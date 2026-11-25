# Nano three-cube qualification, third release

The frozen controller completed every transfer on **46/50** fresh registered layouts. The release requirement was 45 complete layouts, all 50 verified and zero guard bypasses. The independent replay verified 50 distinct skill executions and counted 0 guard bypasses. The registered gate was **met**. [Registration](registration.json) · [Verdict](verification.json) · [Raw evidence](evidence.zip) · [Archive digest](archive-sha256.txt)

The 95% Wilson interval for three-cube success was 81.2%–96.8%. The red cube met the contact and settling criteria in 50 layouts, green in 48 and blue in 48. These counts include partially completed tasks.

The evaluator measured 1,390,551 wheel-supported ticks across 1,390,552 simulation ticks, 0 rejected motor writes, and robot–furniture contact in one layout. The maximum measured excursion beyond a joint limit was 0.00000 rad. [Machine-readable totals](outcomes.json)

## Failed layouts

| Registered case | Cubes meeting all criteria | Measured reason |
| --- | --- | --- |
| 022 | red, blue | The green cube settled 2.62 cm inside the table edge. The required clearance was greater than 3.50 cm. |
| 025 | red, green, blue | All three cubes settled correctly. The wheel-support check failed for one of 24,304 ticks. |
| 040 | red | After braking, Nano could not confirm both its placement heading and the carried cube. Green remained attached and recorded 24 of the required 100 final tabletop-contact ticks. Blue recorded no airborne grip or tabletop contact. |
| 049 | red, green | Stereo docking timed out. The evaluator recorded finger and wrist-camera contact with the coffee table. Blue remained attached and recorded zero final tabletop-contact ticks. |

The first two releases scored 44/50 and 39/50 on their respective cohorts. The third cohort shared zero layout seeds with either of them. The matched development comparison used 17 disclosed second-release layouts and 12 disclosed first-release layouts. An independent extraction reproduced the verdict from the raw evidence, including the motor journals and skill receipts. [Original release](../outcomes.md) · [Second release](../release-2/outcomes.md) · [Matched development trials](../placement-recovery/README.md)

Studio reopened the archived cohort in the native Evidence inspector at both supported window sizes. [1024 × 768](case-evidence-1024.png) · [1440 × 900](case-evidence-1440.png) · [Window and measurement checks](native-offscreen.json)

On a signed Makepad DevHub worker, J28 reopened case 0 and stepped through two recorded exposures. Both RGB digests matched the offscreen review. J29 independently replayed the registered verdict, rejected an untrusted signing identity and detected altered evidence. Signing checks used an ephemeral test identity. An independent operator decision is pending. [Native case inspection](../../../devhub/r3-nano-inspector/case-evidence/verification.json) · [Native qualification review](../../../devhub/r3-nano-review/nano/verification.json)
