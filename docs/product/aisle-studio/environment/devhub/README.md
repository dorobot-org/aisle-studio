# Run native Studio checks with Makepad DevHub

Submit from the AISLE Studio checkout to a registered macOS worker. Use the `makepad-devhub` and `makepad-remote` binaries from the Makepad revision in `Cargo.lock`. The worker needs the [Studio build dependencies](../../../../../README.md#run-locally), an AISLE checkout at the pinned revision, its Python environment and an available Nexus Metal device. Set the variables below to absolute paths on the worker. Each lease needs a fresh evidence directory. The submitting account needs its configured DevHub fleet identity.

```sh
export DEVHUB_AGENT=aisle-studio
makepad-devhub nodes
makepad-devhub job submit --kind test --needs os=macos --node "$NODE" \
  --lane aisle-studio-ui --cost medium --cores 3 --ram-gb 10 \
  --native 'Exercise the Makepad macOS backend and Nexus Metal physics' -- \
  /usr/bin/env CARGO="$CARGO_ON_NODE" CARGO_BUILD_JOBS=3 \
  DOROBOT_AISLE_ROOT="$AISLE_ON_NODE" PYTHONPATH="$AISLE_ON_NODE/src" \
  DOROBOT_AISLE_PYTHON="$AISLE_PYTHON_ON_NODE" \
  /usr/bin/python3 tools/check_devhub_pages.py \
  --remote "$MAKEPAD_REMOTE_ON_NODE" --output "$RESULTS_ON_NODE"
```

DevHub returns the job ID and the submitted commit. `makepad-devhub job result "$JOB_ID" --tail` shows the assigned node, exit code and elapsed time. The script builds the lease checkout with `cargo build --locked`, copies the executable into the evidence directory, drives the native controls using `makepad-remote`, and verifies the executable digest again at the end. `verification.json` records the source commit, binary digest and the SHA-256 of each underlying receipt. Results stay on the assigned worker.

The default job imports photos, saves measured assets, composes a room, runs G1 with named start regions, repeats Nano pickups, cancels a batch, revises and replays trials and checks the four combinations of robot trace and agent history. Nano uses the reference controller by default. Pass `--provider codex` or `--provider octos` to select an authenticated author for its batch.

Append `--suite scene-ranges` to the Python command to check the saved lighting, friction, position and yaw ranges with both robots. Give the lease a fresh output directory. The native checks compare preview hashes to executed samples and measure the resulting Nexus scene. Nano also has to lift and settle the red cube on each seed. [Scene experiment controls](../experiments.md)

For a two-machine check, pass `DOROBOT_AISLE_WORKERS` to the job environment and append `--suite experiment-worker --worker lab-mini --incompatible-worker legacy-worker` to the Python command. Use registered aliases for the current worker and an earlier worker without the experiment protocol. The signed DevHub lease builds and drives the native app. Studio sends the frozen trials to the selected Nexus worker over SSH. The check closes the test app and terminates its own batch supervisor during an active remote lease, then reopens Run and verifies the original lease, returned archive and recording channels. [Worker registry and setup](../experiments.md#run-on-a-registered-worker)

For a focused survey job, give the submission a separate lane and evidence directory and append `--photo-reviews` to the Python command. Both Codex and Octos must be available on the worker's `PATH`, with authenticated provider profiles configured there. Set `DOROBOT_OCTOS_PROFILE` in the job environment when using a named profile. The job imports its own survey and drives both proposal workflows through native Makepad controls. [Photo review and expected measurements](../README.md#review-a-model-proposal)

## Measured leases

The DevHub scheduler and its signed UI worker ran on the same Apple Silicon Mac. The registered experiment worker used a separate Mac mini for the Nexus simulations. DevHub scheduling across two Macs still requires a reachable private fleet connection.

| Lease | Source | Checks | Result |
| --- | --- | --- | --- |
| J1 / L1 | `b80a505` | Two G1 trials of 2,000 physics steps with seeded regions on the floor and a rotated tabletop | Exit 0 in 53 s |
| J3 / L3 | `a34aa95` | Six checks covering native pages, two 2,000-step G1 trials, four successful Nano pickups of 11,266 steps each, durable cancellation of seed 46, original and edited reruns, and four recording combinations | Exit 0 in 822 s. [Binary and receipt digests](native-pages/verification.json) · [Nano trials](native-pages/nano/verification.json) · [Evidence replay](native-pages/evidence/verification.json) |
| J4 / L4 | `0bf2545` | An empty survey was populated using native controls. Codex and DeepSeek via Octos each reviewed two photos. Both runs restored the expected 0.24 m width and retained all earlier revisions | Exit 0 in 76 s. [Binary and receipt digests](photo-reviews/verification.json) · [Codex](photo-reviews/codex/verification.json) · [DeepSeek](photo-reviews/octos-deepseek/verification.json) |
| J6 / L6 | `5d862a1` | DeepSeek via Octos prepared the 30° G1 stance in an empty run history, then revised a retained failed trial in a second workspace. Both nominal reaches passed, and the revised controller and verdict reopened | Exit 0 in 52 s. [Binary and receipt digest](g1-standing/verification.json) · [Measured poses and source failure](g1-standing/g1-revision/verification.json) |
| J7 / L7 | `ccac942` | G1 passed both seeds. Nano placed the red cube on seed 42 and timed out during stereo docking on seed 43 | Cancelled after both Nano trials reached a verdict. [Original failure and camera exposure](../experiment-verification/nano-dim-oak/README.md) |
| J8 / L8 | `a5c6ce4` | G1 passed both 2,000-tick reaches. Nano placed the cube on both seeds with 100 final contact ticks each. Seed 42 reproduced the original motor targets. Seed 43 reproduced the repaired controller’s motor targets | Exit 0 in 217 s. [Binary and receipt digests](scene-ranges/verification.json) · [G1 trials](scene-ranges/ranges/verification.json) · [Nano trials](scene-ranges/nano-ranges/verification.json) |
| J9 / L9 | `95bd58c` | Both robots reached terminal physics verdicts. A previous test window could still write to the shared UI report during the reconnect check | Exit 1 in 410 s. Superseded by J10. |
| J10 / L10 | `b901b6e` | The original Studio process exited and its batch supervisor stopped during an active remote lease for each robot. Run reopened on the same lease. All four remote trials passed and both recording channels were verified after import | Exit 0 in 351 s. [Binary and receipt digests](experiment-worker/verification.json) · [G1](experiment-worker/g1/verification.json) · [Nano](experiment-worker/nano/verification.json) · [Matched local and remote inputs](experiment-worker/matched-inputs.json) |
| J11 / L11 | `7442f2c` | Codex and DeepSeek each proposed a two-part edit, Studio resolved the ambiguous chair and retained the operator’s pose adjustment. Both G1 standing reaches passed, and Nexus measured the reviewed pose | Exit 0 in 80 s. [Binary and receipt digests](scene-agents/verification.json) · [Codex](scene-agents/codex/verification.json) · [DeepSeek](scene-agents/octos-deepseek/verification.json) |

The Evidence run captured 563 frames from each of the head, left-wrist and right-wrist cameras. Nexus measured the chair moving 0.30 m in the variant. The original rerun reproduced the parent verdict digest, and all 1,765 frozen parent artifacts retained their original bytes. [Trial receipts](native-pages/evidence/verification.json) · [Recording combinations](native-pages/channels/channel-verification.json)

To repeat the G1 first-preparation and revision checks, use the submission command above with a fresh evidence directory and append `--g1-revision --provider octos` or `--g1-revision --provider codex`.

To repeat the scene-edit check, append `--suite scene-agents` and give the signed worker access to its configured Codex and Octos CLIs. Each provider runs in a fresh Studio workspace. The job compares the original and revised room, checks undo, redo, discard and reopen, then executes the bound G1 task and reads the chair transforms back from Nexus.
