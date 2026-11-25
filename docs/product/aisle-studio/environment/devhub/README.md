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

For a focused survey job, give the submission a separate lane and evidence directory and append `--photo-reviews` to the Python command. Both Codex and Octos must be available on the worker's `PATH`, with authenticated provider profiles configured there. Set `DOROBOT_OCTOS_PROFILE` in the job environment when using a named profile. The job imports its own survey and drives both proposal workflows through native Makepad controls. [Photo review and expected measurements](../README.md#review-a-model-proposal)

## Measured leases

The scheduler and signed worker ran on the same Apple Silicon Mac. The earlier native headless runs on a separate Mac mini used SSH. Scheduling a lease across two Macs still requires a reachable private fleet connection.

| Lease | Source | Checks | Result |
| --- | --- | --- | --- |
| J1 / L1 | `b80a505` | Two G1 trials of 2,000 physics steps with seeded regions on the floor and a rotated tabletop | Exit 0 in 53 s |
| J4 / L4 | `0bf2545` | An empty survey was populated using native controls. Codex and DeepSeek via Octos each reviewed two photos. Both runs restored the expected 0.24 m width and retained all earlier revisions | Exit 0 in 76 s. [Binary and receipt digests](photo-reviews/verification.json) · [Codex](photo-reviews/codex/verification.json) · [DeepSeek](photo-reviews/octos-deepseek/verification.json) |
