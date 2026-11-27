# Apple Silicon release candidate

The DMG contains a single relocatable AISLE Studio app. Its Python runtime, pinned AISLE source and harness, Nexus Metal wheel, robot models, shared media libraries and FFmpeg tools travel with the app. The bundled reference controller, simulation, episode export and replay work offline. Cloud authoring requires the selected provider's CLI, credentials and connectivity.

Build on Apple Silicon with Xcode command-line tools, Rust, uv, pkg-config, git and Python 3.12 or later. The builder downloads FFmpeg and hash-checked Python wheels, fetches any missing G1 meshes and checks out the pinned AISLE revision. A local checkout containing that revision can be supplied with `--aisle /path/to/AISLE`. Build from a committed Studio revision.

```sh
python3 tools/release/build_macos.py --build /tmp/aisle-studio-rc1 --out dist
```

Mount the DMG, copy the app to a fresh directory outside the checkout and test the installed executable. Supply the matching Makepad remote instrumentation CLI as the test driver. Each test session needs a fresh directory.

```sh
python3 tools/check_headless.py --installed --robot G1 \
  --studio "/Applications/AISLE Studio.app/Contents/MacOS/aisle-studio" \
  --remote "$(command -v makepad-remote)" --session /tmp/aisle-installed-g1
```

Repeat with `--robot Nano` and a different session directory. The check denies access to the Studio checkout, local AISLE checkouts, Homebrew and the managed build Python. External network access and writes to the installed app are blocked. It checks the bundled AISLE adapter hash and revision against the recorded simulation, exports a LeRobot episode, opens Replay, restarts, and verifies the saved result.

The version, component pins, minimum OS and source revisions are recorded in `Contents/Resources/release.json`. Verify the disk image with its adjacent SHA-256 file. This local RC is ad-hoc signed and has no Apple notarization ticket.
