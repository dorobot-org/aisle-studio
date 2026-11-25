# Survey photos in standalone AISLE Studio

Open **Scene → Survey & assets**. Choose **Import photos** to select multiple JPEG or PNG images, or drop files onto the photo area. Select a thumbnail to inspect a photo. Previous and Next navigate through the whole library. Import errors name the affected file.

Studio copies the original bytes to the project, applies the image’s recorded orientation to its preview, and records the source filename, original digest and oriented pixel dimensions. Importing the same filename and contents again reuses the saved photo. Other photos with the same filename can coexist. The library reopens from the project after the originals move.

[Native offscreen capture showing two imported photos and a rejected file](imported-photos.png) · [Generated design and internal review](design-review.md)

## Verify through native input

Build the app as described in the [repository setup](../../../../README.md) and use a Makepad remote client built from the same revision. The test opens Scene, sends native file-drop events, browses photos, moves the source files, restarts Studio and checks both stored originals byte for byte. It exercises the file-drop path shared with picker imports. Interacting with an operating system file picker requires a desktop session.

```sh
python3 tools/check_survey.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --backend headless --session /tmp/aisle-survey-headless
python3 tools/check_survey.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --backend offscreen --session /tmp/aisle-survey-offscreen
```

Use a fresh session directory for each run. This check covers the photo-import portion of the survey-to-asset workflow. Editing photo annotations, defining physical assets and executing a task in an authored scene have their own [release criteria](../PRD.md#end-to-end-release-acceptance).
