# Run a G1 standing reach in an authored room

Select **Unitree G1** on Task. Open **Select scene** or select a saved room in **Scene editor → Create task**. Choose left or right shoulder pitch, a signed displacement from 0.03 to 0.15 rad and the bounded fixture parameters. The permitted block mass is 0.20–0.30 kg, speed 1.10–1.30 m/s and lateral offset −0.015 to +0.015 m. Studio checks these values before saving.

Choose **Use this task → Return to task** and prepare the controller. Start the simulation from **Run**. Studio preserves separate active scene tasks for Nano and G1. Choosing one robot restores its own task and revision.

AISLE loads the actual 29-DOF G1 URDF with a free pelvis. The room and every authored collider share the floor height derived from the neutral soles. The local controller updates targets using joint encoders and the IMU. The operator camera records the test. The evaluator measures joint limits, wrist position, loaded feet, room contacts, physical block contacts, stable recovery and estimated actuator work.

## Native reference run

On the saved five-object living room, headless Makepad on a second Mac ran the reference controller. The local offscreen app prepared the same shoulder target with DeepSeek via Octos. The test rejected an out-of-range mass, saved a 0.23 kg, 1.17 m/s nominal impact and independently read back 14 Nexus bodies. Each run sustained two-foot support throughout 2,000 scored steps. The final wrist error was about 6.3 mm against a 10 mm tolerance. The operator-camera recording contains 100 frames. The test reopened the task and verified that switching to Nano preserved its independent binding. [Headless receipt](g1-verification/reference-headless.json) · [DeepSeek receipt](g1-verification/octos-offscreen.json) · [G1 task form](g1-verification/standing-binding.png)

Codex also prepared the nominal shoulder controller in native Studio. The batch recorded 100 operator-camera frames and 2,000 physics samples, each with all 29 joint measurements and commands. Native Evidence reopened the trial and displayed the camera and trace at physics step 40 (0.20 s). [Codex receipt](g1-verification/codex-offscreen.json) · [Native recording inspector](g1-verification/standing-recording.png)

## Preregistered impact suite

The authored-room baseline **passed 17 of 20** impacts. It passed all eight envelope corners and nine of twelve sealed interior cases. The independent scorer verified all 20 executions, with zero position or rate guard bypasses, support from both feet on all 40,000 scored steps and at most 6.269 mm settled wrist error. The release requirement is 20 of 20.

Cases 13 and 14 had torso contact followed by prolonged left-ankle contact. Case 17 contacted the right ankle and missed the torso. Each lacked the required stable interval after the last impact. These failures are included in the published denominator. The verification archive holds every recipe, opening, frozen task and controller, physical contact log, full-rate actuator and wrist traces, result and source fingerprints. [Preregistration](g1-verification/suite.json) · [All 20 outcomes](g1-verification/report.json) · [Raw evidence archive](g1-verification/g1-room.zip)

## Controller revision on the same impacts

DeepSeek via Octos received the measured failure from case 13 and authored a controller with a −0.10 rad trunk pitch. The shoulder target, room, twenty impact recipes and scoring criteria stayed fixed. The independent verifier reconstructed the verdicts from the archived traces.

| Controller | Passed | Both feet loaded | Worst settled wrist error | Actuator guard bypasses |
| --- | ---: | ---: | ---: | ---: |
| Neutral baseline | 17/20 | 40,000/40,000 steps | 6.269 mm | 0 |
| Revised trunk pitch | 15/20 | 40,000/40,000 steps | 3.055 mm | 0 |

The revision recovered cases 13 and 17. It lost cases 1, 5, 7 and 18 on the impact and recovery criterion. Case 14 failed with both controllers. The neutral controller is the reference option for this room. [Matched manifest](g1-verification/matched-upright/suite.json) · [All measured outcomes](g1-verification/matched-upright/report.json) · [Revised controller and raw traces](g1-verification/matched-upright/matched.zip)

## Qualified 30° left waist turn

For the surveyed living room and the +0.10 rad left shoulder reach, choose **30° left stance** on Task and then **DeepSeek via Octos** or **Ask Codex**. The instruction field shows the request before preparation. To revise a measured run, select it in Evidence, return to Task, enter a posture instruction and choose **Revise with DeepSeek** or **Revise with Codex**. Studio retains the instruction with the controller. The saved scene task pins the room, shoulder and displacement.

DeepSeek via Octos received case 14 from the unsuccessful upright-pitch revision and the operator's request to try the 30° turn. Both cohorts used the same prepared controller and the original scoring criteria. Both manifest hashes were committed before execution. The matched cohort reused all original recipes. The fresh cohort used the same eight envelope corners and twelve newly sealed interior recipes. [Frozen controller and request](g1-verification/turned-stance/controller.json) · [Source failure measurements](g1-verification/turned-stance/feedback.json)

| Cohort | Passed | Both feet loaded | Worst settled wrist error | Recovery after last contact | Guard bypasses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Matched | 20/20 | 40,000/40,000 steps | 5.147 mm | 0.50 s | 0 |
| Fresh | 20/20 | 40,000/40,000 steps | 5.147 mm | 0.50 s | 0 |

The motor traces recorded a 30° yaw command and neutral pitch command at every scored step. Each run's final 100 samples averaged 29.93° of measured waist yaw. The verifier reconstructed wrist, foot, contact, actuator and room checks from both archives. This qualifies the specified simulated room, reach and disturbance envelope. [Matched results](g1-verification/turned-stance/paired/report.json) · [Fresh results](g1-verification/turned-stance/fresh/report.json) · [Matched verification](g1-verification/turned-stance/paired/verified.json) · [Fresh verification](g1-verification/turned-stance/fresh/verified.json)

## Reproduce

Use the pinned AISLE and Nexus runtime and the G1 meshes fetched with `python3 tools/fetch_robot_meshes.py g1`. The native script seeds the same measured asset library and performs profile selection, binding, execution, replay and reopen through Makepad. Use an empty session directory.

```sh
python3 tools/check_g1_scene.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/g1-room --backend headless
python tools/aisle_workbench/g1_reach_suite.py verify \
  docs/product/aisle-studio/environment/g1-verification/g1-room.zip \
  5aaa5d731ff66498a3750c080eec13de555d9e596b0ed4fc5d4bedbed7351488
python tools/aisle_workbench/g1_reach_suite.py verify \
  docs/product/aisle-studio/environment/g1-verification/matched-upright/matched.zip \
  76a8e6506e1fef5da91c2034da8981328424907ca1a71644f6542353a47052a0
```

For a new qualification, pass the prepared G1 `controller.json` to `g1_reach_suite.py prepare`. Record its returned manifest hash before calling `run`. The registered controller and room stay fixed while the suite varies the impact recipe across its declared envelope.

Verify the two turned-stance archives from the repository root with the configured AISLE Python environment.

```sh
G1_RECORDS=docs/product/aisle-studio/environment/g1-verification/turned-stance
python tools/aisle_workbench/g1_reach_suite.py verify "$G1_RECORDS/paired/trials.zip" \
  8b5ad1c7baf93618106e96381cb4ff62f1998445004e20efed6ec6a8b4f91249
python tools/aisle_workbench/g1_reach_suite.py verify "$G1_RECORDS/fresh/trials.zip" \
  7ee4333faf51949a2914ed9f939528006d6088f87d1ee43a4bdac1b497c62fd9
```
