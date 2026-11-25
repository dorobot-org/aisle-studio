# Compose a versioned scene in AISLE Studio

Open **Scene → Scene editor**. Start from **New** or select a saved scene and choose **Open**. Enter the room width and depth in metres and choose **Set**. The grid is spaced every half metre. Switch between the 3D and top views to inspect clearance or place objects in plan.

- **Place an asset.** Select it in the palette and drag the strip immediately below the selection into the viewport. **Place at centre** adds it at the origin. Use **Refresh** after adding a new asset in Survey & assets.
- **Compose from a request.** With coffee table and chair assets in the library, enter “Place one coffee table and two chairs in the room” and choose **Preview**. Inspect the proposed objects and choose **Apply proposal** or **Discard proposal**. The local matcher handles quantities, exact names and requests such as “Move the left chair beside the ottoman.” For an ambiguous name it lists the matches. It currently uses these local commands without calling a model.
- **Adjust an instance.** Select its rendered collider or choose its name under **Scene objects**. Drag it on the floor plane, or enter a position in metres and a yaw in degrees and choose **Apply pose**. **Duplicate**, **Delete**, **Undo** and **Redo** operate on instances. With the viewport focused, the arrow keys select an instance, Delete removes it and Command-Z restores the previous layout.
- **Check or snap.** In the inspector, choose the floor or a named placement surface and choose **Snap**. The editor checks each pinned asset’s colliders for overlaps, room boundaries, floor penetration, missing scale and broken placement surfaces. **Inspect issue** selects an affected object. Pinning an unfinished asset leaves its position visible as a cross.
- **Save or fork.** Give the scene a name and choose **Save revision**. Later saves preserve the previous snapshots. To create a separate scene, change the name and choose **Save As**. The variant records its source scene and revision. Reopen the original from the scene library.

[Native proposal preview](scene-verification/proposal.png) · [Five placed instances](scene-verification/five-instances.png)

## Reproduce the native test

The test begins with a small metric fixture library containing a table, chair, cube and ottoman. The chairs and ottoman use box colliders. It previews and applies three instances, drags in two more, moves a chair using its rendered collider, deletes it and undoes the deletion. After restarting Studio, it previews moving the left chair, saves a separate variant and reopens the original. The test independently checks identities, pinned revisions, poses, variant parent and the digest of the original scene.

```sh
python3 tools/check_scene_editor.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --backend headless --session /tmp/scene-editor-headless
python3 tools/check_scene_editor.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --backend offscreen --session /tmp/scene-editor-offscreen
```

Use a fresh directory per run. The two receipts store the actual base and variant scenes. The geometry checks cover authored furniture. Robot start clearance and execution in Nexus belong to the scene-bound task qualification. [Headless receipt](scene-verification/headless.json) · [Offscreen receipt](scene-verification/offscreen.json) · [Release acceptance](../PRD.md#end-to-end-release-acceptance)
