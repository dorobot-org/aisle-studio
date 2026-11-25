"""Exercise photo annotation, measured geometry, and revisions through native UI input."""

import hashlib
import json


def exercise(target, act, wait, shows, folder, backend):
    pane = "aisle_scene.project_survey."

    def click(name):
        target(pane + name)

    def edit(name, text):
        click(name)
        act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        act("text", "w=0", "t=" + text, "wait=1")

    def mark(point, kind="click"):
        control = pane + "photo_canvas.video_area.frame_image"
        state = wait(lambda d: d["controls"][control]["rect"][2] > 0)
        x, y, w, h = state["controls"][control]["rect"]
        px, py = point
        args = ("w=0", f"x={x+w*px/1536}", f"y={y+h*py/1024}", "wait=1")
        if kind == "click":
            act("click", *args)
        else:
            act("mouse", "k=" + kind, *args)

    def snapshot(name):
        if backend == "offscreen":
            act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    click("thumb0.select")
    shows("file_label", "room.jpg")
    edit("room_name", "Living room")
    click("assign_room")
    shows("import_result", "belongs to Living room")
    click("edit_assets")
    edit("asset_name", "Coffee table")
    edit("category", "Furniture")
    # Perimeter of the tabletop in the generated 1536 × 1024 source.
    for xy in [(155, 309), (510, 214), (1353, 279), (1223, 438)]:
        mark(xy)
    mark((155, 309), "down")
    mark((188, 327), "move")
    mark((188, 327), "up")
    click("floor_ref")
    for xy in [(211, 752), (1181, 901)]:
        mark(xy)
    click("dimension")
    for axis, pair, length in [
        ("Width", [(188, 327), (1223, 438)], "80"),
        ("Depth", [(1223, 438), (1353, 279)], "0.5"),
        ("Height", [(1197, 447), (1181, 900)], "45"),
    ]:
        shows("axis_label", axis)
        for xy in pair:
            mark(xy)
        if axis == "Depth":
            click("length_unit")
        unit = "m" if axis == "Depth" else "cm"
        edit("length_value", length)
        click("apply_length")
        shows("measured_values", f"{length} {unit} · measured")
        if axis != "Height":
            click("next_axis")
    snapshot("table-measurements")
    click("edit_physics")
    click("asset_shape")
    shows("geometry_info", "5 colliders")
    edit("material", "Oak")
    edit("surface_name", "Coffee tabletop")
    click("preview_physics")
    shows("geometry_info", "Coffee tabletop")
    click("save_asset")
    shows("asset_version", "Coffee table · revision 1")
    snapshot("table-colliders")
    # Saving another exposure against the same asset leaves the first revision intact.
    click("thumb2.select")
    shows("file_label", "table-leg.jpg")
    click("edit_marks")
    for xy in [(50, 321), (918, 16), (1410, 246)]:
        mark(xy)
    click("save_asset")
    shows("asset_version", "Coffee table · revision 2")
    click("new_asset")
    edit("asset_name", "Red cube")
    edit("category", "Graspable object")
    for xy in [(676, 772), (752, 748), (847, 758), (840, 902), (674, 910)]:
        mark(xy)
    click("edit_physics")
    click("movable")
    click("graspable")
    edit("mass", "0.09")
    click("save_asset")
    shows("asset_version", "Red cube · revision 1")
    snapshot("cube-mass")
    return verify_files(folder)


def verify_files(folder):
    root = folder / "state/aisle/survey"
    photos = {
        json.loads(p.read_bytes())["filename"]: p.parent.name
        for p in (root / "photos").glob("*/photo.json")
    }
    assert (
        json.loads((root / "photos" / photos["room.jpg"] / "room.json").read_bytes())
        == "Living room"
    )
    assert not (root / "photos" / photos["table-leg.jpg"] / "room.json").exists()
    versions = [p for p in (root / "assets").glob("*/*.json")]
    assets = {
        (a["name"], a["revision"]): (a, p)
        for p in versions
        for a in [json.loads(p.read_bytes())]
    }
    assert set(assets) == {
        ("Coffee table", 1),
        ("Coffee table", 2),
        ("Red cube", 1),
    }, list(assets)
    first = assets[("Coffee table", 1)][0]
    last = assets[("Coffee table", 2)][0]
    cube = assets[("Red cube", 1)][0]
    assert first["id"] == last["id"] != cube["id"]
    assert set(first["photos"]) == {photos["room.jpg"]}
    assert set(last["photos"]) == {photos["room.jpg"], photos["table-leg.jpg"]}
    mark = first["photos"][photos["room.jpg"]]
    assert len(mark["outline"]) == 4 and len(mark["floor"]) == 2

    def near(p, q):
        return max(abs(i - j) for i, j in zip(p, q)) < 4

    assert near(mark["outline"][0], (188, 327)), mark
    assert near(mark["outline"][2], (1353, 279)) and near(mark["floor"][1], (1181, 901))
    assert [d["value"] for d in first["dimensions"]] == [80, 0.5, 45]
    for i, d in enumerate(first["dimensions"]):
        assert d["unit"] == ["cm", "m", "cm"][i] and d["provenance"] == dict(
            source="measured", photo=photos["room.jpg"], endpoints=mark["rulers"][i]
        )
    # Compare metric geometry to independent tape entries, not to the runtime generator.
    table = first["geometry"]
    top, *legs = table["colliders"]
    assert top["size_m"] == [0.8, 0.5, 0.035] and len(legs) == 4
    assert table["surfaces"] == [
        dict(name="Coffee tabletop", center_m=[0.0, 0.0, 0.45], size_m=[0.8, 0.5])
    ]
    assert all(
        l["size_m"][2] < 0.45
        and abs(l["center_m"][0]) > 0.3
        and abs(l["center_m"][1]) > 0.19
        for l in legs
    )
    assert first["physics"]["material"] == dict(
        value="Oak", provenance=dict(source="operator")
    )
    assert cube["physics"]["mass_kg"] == dict(
        value=0.09, provenance=dict(source="operator")
    )
    assert cube["physics"]["movable"]["value"] and cube["dimensions"] == [None] * 3
    assert cube["physics"]["graspable"] == dict(
        value=True, provenance=dict(source="operator")
    )
    assert first["physics"]["surface_name"] == dict(
        value="Coffee tabletop", provenance=dict(source="operator")
    )
    assert last["geometry"] == first["geometry"]
    return {
        str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in versions
    }


def restored(target, shows, folder, original):
    pane = "aisle_scene.project_survey."
    shows("room_name", "Living room")
    target(pane + "edit_assets")
    shows("asset_version", "Coffee table · revision 2")
    shows("measured_values", "80 cm · measured")
    target(pane + "edit_physics")
    shows("geometry_info", "5 colliders")
    target(pane + "next_asset")
    shows("asset_version", "Red cube · revision 1")
    assert verify_files(folder) == original
