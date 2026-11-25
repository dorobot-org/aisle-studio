# Nano three-cube skill qualification

The skill transferred all three cubes on **44 of 50** predeclared layouts. The requirement was 45 successful layouts, 50 verified motor journals and zero guard bypasses. All 50 cases ran through distinct Octos requests, and an independent replay recovered the same verdict. [Registration](registration.json) · [Machine-readable verdict](verification.json)

The evaluator recorded 1,344,444 wheel-supported ticks across 1,344,462 physics ticks and 0 guard bypasses. The red cube met all lift, release, support and settling criteria in 50 cases, green in 48 and blue in 45. Per-color counts include the successful transfers in partially completed layouts.

## Failed layouts

| Registered case | Measured outcome |
| --- | --- |
| 007 | Red and green settled. The wrist camera could not confirm a blue grasp in two attempts. Blue had zero airborne grip ticks. |
| 016 | Blue spent 5,898 ticks airborne between opposing fingers and recorded zero of the required 100 final tabletop contacts. |
| 020 | Red settled. Two green grasp attempts failed wrist confirmation. Neither green nor blue recorded an airborne grip. |
| 034 | Blue spent 3,150 ticks airborne between opposing fingers and recorded zero final tabletop contacts. |
| 043 | Stereo docking at the coffee table timed out. The fingers and wrists contacted the table during carry and the wheels lost support for 18 ticks. |
| 048 | Green spent 3,302 ticks airborne between opposing fingers and recorded zero final tabletop contacts. |

## Native review and replay

The held-out batch used sparse observation mode. The replay archive contains the motor journal, calibration, actions, task measurements and Octos receipts for each case. Continuous three-camera video was disabled for this batch.

Studio reopened the recorded cases in the offscreen app and signed Makepad DevHub job J15. Both workflows rejected an untrusted signer and a modified measurement, then reopened the test decision after a restart. The product evaluation has no independent reviewer signature. [Offscreen check](native-offscreen.json) · [Headless check](../../devhub/skill-review-nano/nano/verification.json) · [Headless build manifest](../../devhub/skill-review-nano/verification.json)

[Narrow review screen](skill-review-1024.png) · [Wide review screen](skill-review-1440.png) · [Measured control bounds](window-sizes.json)

Use the [G1 restore instructions](../README.md#replay-a-recorded-cohort) with the following Nano evidence values.

- Registration `7bea0963a8c9e4e90c53db6e9e55190ac9d3d4c7c50c981f19e4b8b5e3a5bb8b`
- Archive `nano/evidence.zip` with SHA-256 `937b13ef8697e6fc1c231d662e671c6a431fd13891fbd10dca91c377ce0d7e25`
