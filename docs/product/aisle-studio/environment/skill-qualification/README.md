# Qualify a controller and review the evidence

In **Task**, choose an installed Octos skill and prepare a controller. For Nano, use the three-cube reference living room and request transfer of all three colors to the coffee table. For G1, select a standing shoulder reach. The measured G1 example uses a 30° left waist turn in the saved living room. Both recorded controllers were authored by DeepSeek via Octos. The local controller runs each physics episode using the pinned Octos package. [Package setup](../spec-skills.md)

1. Open **Evidence → Skill qualification → Register current controller**. Record the displayed registration digest in the review system before anyone runs the cohort. The public portion contains the physics and software identities, thresholds and salted case commitments. The openings are held in the local project.
2. Select **Run remaining cases**. The worker binds every trial to the same package, controller, robot and observation contract. A started slot stays in the denominator after interruption. A restart can run the other slots. **Refresh** updates the selected registration.
3. Select **Verify measurements**. For Nano, Studio replays the motor journal, checks any recorded release exposures, and independently applies lift, detachment, support and joint criteria. For G1, Studio reconstructs wrist travel, foot loads, recovery and actuator limits from the stored traces. Each case needs its own Octos request, run ID and Nexus measurement. Evidence lists failed case indices and reasons.
4. Configure the reviewer's identity, enter a rationale, and choose **Approve simulation** or **Reject simulation**. Studio replays the raw evidence again and checks the OpenSSH signature against the installation's allowed signers. An approval requires the registered threshold and verified traces for every case. A later signed rejection supersedes the prior approval for this project.
5. To pass the result to another team, select the reviewed controller in **Task** and choose **Export signed handoff**. The dossier binds the registration, verified verdict and decision history. Physical-device calibration, telemetry loss, stop behavior and supervised motion have a separate acceptance record.

## Recorded simulations

Both registrations reached the remote PR branch before execution. Members with repository access can compare the disclosed openings against those hashes. [Preregistration commit](https://github.com/dorobot-org/aisle-studio/commit/0b0c6ab79a04646dfea2476bc4e67ed66b622311)

| Profile | Registered acceptance | Recorded outcome |
| --- | --- | --- |
| G1 package | All 20 cases must meet the bounded-impact standing reach criteria | 20/20 passed, 20/20 distinct receipts verified, 40,000 scored steps with both feet loaded, zero guard bypasses. Worst settled wrist error was 5.15 mm and the longest stable recovery was 0.50 s. [Registration](g1/registration.json) · [Verification](g1/verification.json) · [Raw archive](g1/evidence.zip) |
| Nano package | Complete all three transfers on at least 45 of the 50 hidden layouts, verify every case and record zero guard bypasses | **44/50** complete three-cube transfers. Below the registered gate. All 50 bridge receipts and motor journals verified with 0 guard bypasses. [Registration](nano/registration.json) · [Per-case outcomes](nano/outcomes.md) · [Raw archive](nano/evidence.zip) |

The G1 archive contains the original unsigned evaluation. The native workflow test used its own ephemeral reviewer key in an isolated copy. An untrusted identity and a modified measurement were both rejected. The test exported a signed simulation dossier, reopened the decision after restart and exercised supersession with a signed rejection. The original project remains available for an independent reviewer. [Native acceptance record](g1/native-offscreen.json) · [1024 × 768](g1/native-review.png) · [1440 × 900](g1/native-review-1440.png) · [Measured control bounds](g1/window-sizes.json)

Signed Makepad DevHub job J13 compiled the release candidate, registered a second G1 cohort from the native Evidence page, and executed all 20 cases through the real bridge. All 20 passed, each receipt verified, and Studio restored the signed review after a restart. The job finished in 335 seconds. [Headless build manifest](../devhub/skill-qualification-g1/verification.json) · [G1 workflow receipt](../devhub/skill-qualification-g1/g1/verification.json) · [J14 replay on the final responsive review layout](../devhub/skill-review-g1/verification.json)

## Inspect a case

Select **Inspect case evidence** on a verified registration to review a specific success or failure. Nano shows measured grip, support and retained release images. G1 shows the impact, two-foot support, wrist error and recovery. [Operator steps and measured screenshots](case-inspector/README.md)

## Test a revision on a disclosed Nano failure

Restore the original Nano archive from its [recorded Studio checkout](https://github.com/dorobot-org/aisle-studio/tree/1762cbc9009e323dd03fb4b14aab979eaee423d8) to the project first. Open the same project in the revised Studio, select **Nano** and prepare its three-cube controller in **Task**. Open **Evidence → Skill qualification**, choose the verified original registration, select a failed layout and click **Test current controller on layout**. Studio runs the prepared controller on the disclosed layout and shows the two outcomes, candidate digest, physics ticks and guard bypasses. Choose a different failure or reopen the project to retrieve the latest comparison for that layout. The original measurements and the new development trials have distinct locations in the project. [Example and camera evidence](nano/wrist-survey/README.md)

## Second release registration

The revised Nano controller uses stationary pickup, one wrist-based reacquisition after two missed grasps, an elevated RGB survey before releasing near previously placed cubes, and the original docking distance. The selected DeepSeek/Octos plan completed 11 of 12 disclosed development layouts, including all six previously passing controls. The new Nano registration commits to 50 fresh layouts disjoint from the original 50. The G1 reference is registered again because its runtime identity binds the shared Studio adapter. Its standing-reach task and acceptance criteria are unchanged. [Development comparison](nano/wrist-survey/README.md) · [Nano preregistration](nano/release-2/registration.json) · [G1 preregistration](g1/release-2/registration.json)

The second Nano release completed **39/50** fresh three-cube layouts. All 50 cases verified with zero guard bypasses, and the quantitative gate of 45/50 was missed. Every scored tick remained wheel-supported. The 11 failures occurred during placement. Nine involved tracking earlier cubes across a turn and two found no reachable clear release location. [All failures and raw archive](nano/release-2/outcomes.md)

The second G1 release met every registered requirement. All 20 standing reaches passed with distinct bridge receipts and zero guard bypasses. Both feet carried load for all 40,000 scored steps. The worst settled wrist error was 5.15 mm and the longest stable recovery took 0.50 s. A separate extraction of the evidence archive reproduced the verified verdict. An independent operator review is pending. [Measured G1 verdict](g1/release-2/verification.json) · [Raw G1 archive](g1/release-2/evidence.zip) · [Archive digest](g1/release-2/archive-sha256.txt)

Signed Makepad DevHub job J17 tested the exact frozen Nano controller on disclosed layout 20. The headless Evidence page completed the transfer, reopened it after restart and correctly restored the selected result when switching between cases. [Build manifest](../devhub/release-2-replay/verification.json) · [Nano receipt](../devhub/release-2-replay/nano-case/verification.json)

Codex and DeepSeek via Octos also received the same instruction and public feedback independently. Each completed two of the three disclosed comparison layouts. Their motor journals were identical in all three pairs, including the docking timeout. [Matched provider measurements](nano/provider-comparison/README.md)

## Third release registration

The selected Nano revision reobserves earlier cubes from a stationary, raised wrist when placement tracking fails. It completed 17/17 development layouts from the second release, recovering 11/11 previously failed layouts and preserving 6/6 successful controls. It also completed 11/12 disclosed layouts from the original cohort. The fresh 50-layout Nano registration shares no seeds with either earlier cohort. G1 repeats the 20-case standing reach because the Studio adapter identity is shared by both profiles. [Development measurements](nano/placement-recovery/README.md) · [Nano registration](nano/release-3/registration.json) · [G1 registration](g1/release-3/registration.json)

The third G1 qualification passed all 20 cases. The verifier measured 40,000 two-foot-supported steps and zero guard bypasses. The native Evidence page reopened and independently checked the same archive on signed DevHub jobs J25 and J26. [G1 results, screenshots and raw evidence](g1/release-3/outcomes.md)

The complete native page regression on the registered source also passed on the signed Makepad worker. The seven checks covered survey inputs, scene editing, G1 starts and randomization, the Nano task, Evidence and recorded data channels. [DevHub J27](../devhub/r3-pages-confirmed/job.json) · [Workflow receipts](../devhub/r3-pages-confirmed/verification.json)

## Reviewer identity

Configure the native Studio process with the two paths below. The signing key identifies the reviewer. The allowed signers file is maintained by the team that owns release decisions. Studio does not create a reviewer identity for an operator. Use an OpenSSH key available to the local signer, including through an unlocked agent. For example, the authorized-signers entry can scope a public key to this protocol.

```text
reviewer@example.org namespaces="aisle-studio-simulation-qualification-v1" ssh-ed25519 PUBLIC_KEY
```

```sh
export DOROBOT_QUALIFICATION_ALLOWED_SIGNERS="$REVIEW_TEAM_ALLOWED_SIGNERS"
export DOROBOT_QUALIFICATION_SIGNING_KEY="$REVIEWER_KEY_FILE"
```

## Replay a recorded cohort

Use the same installed package, AISLE/Nexus runtime and Octos deployment bridge as the registration. Run the following from the Studio checkout with the environment from the [package setup](../spec-skills.md). The state path is the current Studio project's `aisle/qualifications` directory. Restore checks the archive digest, registration, raw measurements and every case verdict in a staging directory, then adds the verified cohort to the project. Select it from the qualification page after **Refresh**.

```sh
"$DOROBOT_AISLE_PYTHON" tools/skill_qualification/archive.py restore \
  --state "$DOROBOT_STUDIO_STATE_DIR/aisle/qualifications" \
  --id a31e1870b2417161f0510d8913e039712b8ae581cdde49bc5da5f5c245267cd9 \
  --archive docs/product/aisle-studio/environment/skill-qualification/g1/evidence.zip \
  --sha256 203c7e52e1f8205432e2457baddd04dfb27e994229b463e7c464f45903e324eb
```

The archived registration binds this version of the evaluator and simulator to the verdict. Reproduce it from the recorded release checkout. Changes to a controller, runtime or review policy require a new qualification or a separately audited migration. For a local run, `archive.py export` takes the same `--state`, `--id` and a new `--archive` path and prints its digest.
