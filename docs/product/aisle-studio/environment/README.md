# Build a measured asset from survey photos

In **Scene → Survey & assets**, import room and object photos with **Import photos** or drop them onto the photo area. Select a thumbnail. Optionally enter the room name and choose **Apply**. Studio keeps its own copy of each JPEG or PNG, its original digest and its oriented pixel dimensions.

1. Choose **Edit assets → New**, enter a name and category, and select **Outline**. Click around the object in the photo. Drag an existing point to correct it. Use **Floor points** to mark a floor reference.
2. Choose **Ruler**. Select width, depth or height, click the two endpoints, enter the tape measurement, select its unit and choose **Apply**. Each axis keeps its own units and source image. A length entered without endpoints is recorded as an operator entry.
3. In **Physics**, select the shape. A table has an editable top thickness, leg width and placement surface name. Choose **Preview changes** and **Rotate** to inspect the slab and four separate leg colliders. A box can be movable and graspable. Enter its mass in kilograms. Missing dimensions stay visible as unmeasured.
4. Choose **Save asset revision**. To link another view, select a second photo with the same asset open, add marks and save again. Each save retains the earlier revision. Use **Previous asset** or **Next** to switch between assets.

[Annotated source photo](survey-verification/table-measurements.png) · [Saved table collision geometry](survey-verification/table-colliders.png) · [Generated design and concept review](design-review.md)

## Reproduce the native acceptance test

Build the app using the [repository setup](../../../../README.md). Use a Makepad remote client built from the same Makepad revision. Each command requires a fresh session directory.

```sh
python3 tools/check_survey.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --assets --backend headless --session /tmp/survey-headless
python3 tools/check_survey.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --assets --backend offscreen --session /tmp/survey-offscreen
```

The test begins with an empty project, drops two JPEGs and a PNG, and checks the error for a damaged file. It assigns a room photo, drags a tabletop outline point, marks the floor, measures three axes, and saves two views of one table. A separate graspable cube receives a mass of 0.09 kg. The test moves the original files, restarts the application and verifies all three image digests and all three immutable asset revisions.

The table measures 80 cm wide, 0.5 m deep and 45 cm high. The saved geometry includes a named placement surface and four legs with clearance between them. The two JPEGs are generated fixtures. These measurements and the cube mass are operator-supplied test inputs. The automated test uses native file drops. The operating system file picker requires a desktop session.

Both native backends passed on November 24, 2026. The headless run used a separate Mac through SSH. Each receipt contains the source image digests, asset revision digests, original units, measurement endpoints, physics settings and metric geometry. [Offscreen receipt](survey-verification/offscreen.json) · [Headless receipt](survey-verification/headless.json) · [Five release tests](../PRD.md#end-to-end-release-acceptance)

## Review a model proposal

In **Scene → Survey & assets → Edit assets**, select a photo, mark the target if needed and open **Suggestions**. Describe the object. **Ask Codex** sends the oriented RGB photo and its region measurements to Codex. **Ask DeepSeek** sends the region outlines, bounds and mean RGB values to the configured Octos profile. The source label identifies the input and model used.

1. Choose a proposal and a property. **Show source photo** opens the original view and highlights the proposed outline in yellow. **Edit outline** copies it into the draft so its handles can be dragged.
2. Choose **Use value** to accept a proposal, type a correction before choosing **Use value**, or select **Discard value**. **Save asset revision** persists each decision, the original suggestion, source photo digest, request digest and model. A corrected value records the operator as its source.
3. For an approximate metric length, use a straight-on photo of a flat target beside a known reference in the same plane. Select the dimension axis, mark both ends of the reference with the ruler and enter its length in metres in **Suggestions**. Studio projects the chosen outline onto that direction. With the length blank, the workflow proposes a name and, when supported, a shape. A perspective room photo can still use the ordinary tape-measure fields.

The acceptance run used a furnished-room fixture and a separate front-on calibration image. It verified three review decisions, preserved previous revisions, moved an outline handle and reopened the accepted 0.24 m width. Both provider runs used the same Mac. [Codex offscreen receipt](survey-verification/proposals-codex-offscreen.json) · [DeepSeek headless receipt](survey-verification/proposals-deepseek-headless.json) · [Native review](survey-verification/proposal-review.png) · [Reviewed width](survey-verification/proposal-width.png)

From the repository root, use the AISLE Python environment, an authenticated provider and a survey created by the native acceptance test above.

```sh
.venv-aisle/bin/python tools/check_asset_suggestions.py \
  --studio target/debug/aisle-studio --remote "$(command -v makepad-remote)" \
  --survey /tmp/survey-headless/state/aisle/survey --provider octos-deepseek \
  --backend headless --session /tmp/survey-deepseek-review
```
