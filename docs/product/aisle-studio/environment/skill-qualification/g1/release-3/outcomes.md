# G1 bounded-impact standing reach, third release

G1 passed **20/20** preregistered cases. The verifier checked 20 distinct skill executions and counted 0 guard bypasses. Both feet carried load for 40,000 scored simulation steps. The worst settled wrist error was 5.15 mm and the longest measured stable recovery took 0.50 s. [Measured totals](outcomes.json) · [Verdict](verification.json)

The saved room, standing posture and bounded impact task use the same controller parameters as the preceding release. This registration records the shared Studio adapter that ships with the revised Nano policy. Independently restoring the evidence archive reproduced the verdict for all 20 cases. [Registration](registration.json) · [Bound task](bound-task.json) · [Raw measurements](evidence.zip) · [Archive digest](archive-sha256.txt)

Studio reopened the archived cohort in the native Evidence inspector at both supported window sizes. [1024 × 768](case-evidence-1024.png) · [1440 × 900](case-evidence-1440.png) · [Window and measurement checks](native-offscreen.json)

On a signed Makepad DevHub worker, J25 opened both a boundary case and an interior case. J26 reverified all 20 trials through the Evidence page, exercised signing and revocation with an ephemeral test identity, reopened the decision after restart and detected a modified measurement. An independent operator decision is pending. [J25 inspection](../../../devhub/r3-g1-inspector/case-evidence/verification.json) · [J26 review](../../../devhub/r3-g1-review/g1/verification.json)
