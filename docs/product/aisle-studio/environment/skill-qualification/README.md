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
| Nano package | Complete all three transfers on at least 45 of the 50 hidden layouts, verify every case and record zero guard bypasses | Evaluation in progress. [Registration](nano/registration.json) · [Layout commitments](nano/suite.json) |

The G1 archive contains the original unsigned evaluation. The native workflow test used its own ephemeral reviewer key in an isolated copy. An untrusted identity and a modified measurement were both rejected. The test exported a signed simulation dossier, reopened the decision after restart and exercised supersession with a signed rejection. The original project remains available for an independent reviewer. [Native acceptance record](g1/native-offscreen.json) · [1024 × 768](g1/native-review.png) · [1440 × 900](g1/native-review-1440.png) · [Measured control bounds](g1/window-sizes.json)

Signed Makepad DevHub job J13 compiled the release candidate, registered a second G1 cohort from the native Evidence page, and executed all 20 cases through the real bridge. All 20 passed, each receipt verified, and Studio restored the signed review after a restart. The job finished in 335 seconds. [Headless build manifest](../devhub/skill-qualification-g1/verification.json) · [G1 workflow receipt](../devhub/skill-qualification-g1/g1/verification.json) · [J14 replay on the final responsive review layout](../devhub/skill-review-g1/verification.json)

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
