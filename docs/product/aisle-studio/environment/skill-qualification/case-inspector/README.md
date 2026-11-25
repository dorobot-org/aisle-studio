# Inspect a qualification case

Open **Evidence → Skill qualification**, choose a verified registration, and select **Inspect case evidence**. The case selector includes successes and failures. Each selection reads the original verified records. The inspector works with a local cohort or a restored review archive.

**G1.** Select an impact to see its mass, speed and lateral offset, the number of two-foot-supported steps, settled wrist error and recovery time. The eight boundary recipes are counted separately from the twelve interior recipes. Case 8 of the second G1 registration is an interior impact of 0.24 kg at 1.194 m/s. The robot maintained support for 2,000 steps and recovered in 0.50 s. [Offscreen view](g1-inspection.png) · [Measured inspection](g1-offscreen.json)

**Nano.** Select a layout to see its seed, elapsed physics ticks, and the opposing-finger lift and final tabletop contact counts for each cube. The cohort estimate includes a 95% Wilson interval when every case has been verified. The original 44/50 cohort gives 76.2%–94.4%. Its controller did not retain release exposures. Opening such a case displays that recording status. [Original-cohort headless inspection](nano-original-headless.json)

When the controller recorded release exposures, **Next exposure** and **Previous exposure** move between sensor timestamps. The image strip shows the head, left wrist and right wrist in that order. The caption identifies the cube, physics tick, simulation time and RGB digest. For a raised-wrist survey it also reports the number of earlier cubes the controller saw. Studio checks the packet against the verified case and renders its original RGB samples.

Signed DevHub job J19 exercised the G1 case selector and showed both sampling groups using the committed native binary. Job J18 reverified the same release in the full native review workflow, including rejection of an unknown reviewer and altered measurement. It used an isolated test identity. [Headless case inspection](../../devhub/g1-case-evidence/case-evidence/verification.json) · [Headless review](../../devhub/g1-release-2-review/g1/verification.json)

## Compare a disclosed retry with its registered outcome

Select a failed Nano layout, run **Test current controller on layout** and choose **Inspect retry observations**. The development view identifies the candidate controller and shows its own cube measurements and release exposures. The original cohort tally stays visible. A retry needs recorded evidence digests for this view. Earlier retries still show their paired pass/fail result.

On layout 20 the frozen candidate completed all three transfers. The operator reopened the retry and inspected the synchronized head and wrist images at ticks 21,000 and 21,240. At the later tick the raised right wrist detected the earlier red cube on the table. The test compared each of the six rendered camera regions against the original sensor packet. The largest mean pixel error was 1.17 levels on the 0–255 scale. [Docked camera views](nano-retry-docked.png) · [Raised wrist view](nano-retry-raised.png) · [Per-camera measurements and receipt](nano-retry-offscreen.json)

Signed DevHub job J20 ran a separate retry through the frozen controller and reproduced the RGB digests of both exposures. The G1 case inspector was also checked at both supported window sizes. [J20 headless result](../../devhub/retry-observations/nano-case/verification.json) · [G1 at 1024 × 768](g1-1024.png) · [G1 at 1440 × 900](g1-1440.png) · [Measured bounds](g1-window-sizes.json)
