# Bind a Nano pickup to a saved scene

1. Set each object’s appearance under **Survey & assets → Physics** and save its asset revision. The Nano controller recognizes red, green and blue pickup objects. Give the pickup a mass and enable both mobility and grasping. Give the fixed destination a named placement surface. A scene instance keeps the asset revision chosen when it was placed.
2. Open a saved scene and choose **Create task**, or choose **Task → Select scene**. Pick the source object and the destination surface. With two graspable objects, Studio waits for an explicit choice. The orange brackets show the pickup and the teal top face shows the placement surface.
3. Choose **Use this task**, then **Return to task**. Select the reference controller, Codex or DeepSeek via Octos. Enable onboard RGB for the agent to inspect observations during preparation. Codex receives the camera images and measurements. Octos receives the RGB-derived measurements in text. All three execute the local visual controller using Nano’s head and wrist RGB cameras and encoders.
4. In **Run**, start the simulation. The saved task contains the instruction, robot, required observations, success criteria, scene revision, pinned asset revisions and instance IDs. The prepared controller records the task digest. The simulation copies those exact inputs into the trial and constructs the room, table colliders and movable boxes in Nexus.
5. Open **Evidence** to inspect the measured trial. Renaming an instance keeps the binding. Selecting a later scene revision that deleted the object marks the reference as missing. An earlier saved revision remains selectable.

[Selected cube and tabletop in native Studio](task-verification/bound-pickup.png) · [Deleted binding in a later revision](task-verification/broken-binding.png)

## What the qualification measures

The fixture contains one measured-size table with five colliders, a red cube and a blue cube. The selected cube is 40 mm wide and has a mass of 32 g. The scene floor is the origin for user-entered dimensions. Nexus uses a wheel contact plane 34.3 mm below the Nano URDF origin. The resulting tabletop is 0.54 m high in Nexus world coordinates.

The evaluator identifies the two selected Nexus bodies independently of the visual controller. It counts loaded opposing-finger contacts while the cube is elevated, checks release clearance, and requires 100 consecutive tabletop contact steps with less than 3 mm of motion. The test also checks wheel support, joint excursions, the motor command audit and the position of the blue bystander. Each receipt distinguishes measured body poses from the collider dimensions and material settings supplied at construction.

The headless reference run on a second Mac maintained wheel support for all 6,848 steps. The DeepSeek run maintained support for all 11,266 steps. Codex maintained support for all 11,266 steps. All three placed the red cube and passed the final 100-step contact check. These are single trials of one authored layout. [Reference result](task-verification/reference-headless.json) · [DeepSeek result](task-verification/deepseek-offscreen.json) · [Codex result](task-verification/codex-offscreen.json)

## Reproduce through Makepad

Use the Python environment, pinned AISLE source and Nexus version from the [workbench setup](../../../../README.md). Each run needs a fresh session directory. The initial scene and asset library are fixtures. All binding, ambiguity resolution, controller preparation, launching, renaming, deleting and reopening are driven through native Studio controls.

```sh
python3 tools/check_bound_pickup.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-bound-reference --backend headless
python3 tools/check_bound_pickup.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-bound-codex --backend offscreen --provider codex
python3 tools/check_bound_pickup.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --session /tmp/nano-bound-octos --backend offscreen --provider octos
```

The agent commands use an already configured local provider. Set `DOROBOT_OCTOS_PROFILE` if the DeepSeek credential belongs to a named Octos profile. Use `--bindings-only` for a native editing regression without starting physics.
