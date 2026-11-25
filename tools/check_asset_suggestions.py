"""Review real provider proposals in native Survey, then reopen the asset revisions."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil

from PIL import Image, ImageDraw
from studio_native import NativeStudio

PANE = "aisle_scene.project_survey."


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--studio", type=Path, required=True)
    parser.add_argument("--remote", type=Path, required=True)
    parser.add_argument("--session", type=Path, required=True)
    parser.add_argument(
        "--survey",
        type=Path,
        required=True,
        help="Survey directory from check_survey.py --assets",
    )
    parser.add_argument(
        "--backend", choices=["headless", "offscreen"], default="headless"
    )
    parser.add_argument(
        "--provider", choices=["codex", "octos-deepseek"], required=True
    )
    args = parser.parse_args()
    folder = args.session.resolve()
    folder.mkdir(parents=True, exist_ok=False)
    library = folder / "state/aisle/survey"
    shutil.copytree(args.survey, library)
    app = NativeStudio(args, folder)
    board = folder / "front-on-calibration.png"
    image = Image.new("RGB", (960, 640), "white")
    draw = ImageDraw.Draw(image)
    draw.rectangle((240, 160, 720, 480), fill="#2346cd")
    draw.line([(240, 535), (480, 535)], fill="black", width=4)
    for x in [240, 480]:
        draw.line([(x, 525), (x, 545)], fill="black", width=4)
    image.save(board)

    def click(key):
        if key not in {"edit_assets", "save_asset", "thumb0.select", "thumb1.select"}:
            app.scroll_to(PANE + key, PANE + "form_scroll")
        return app.click(PANE + key)

    def edit(key, value):
        click(key)
        app.act("key", "w=0", "c=KeyA", "logo=1", "wait=1")
        app.act("text", "w=0", "t=" + value, "wait=1")

    def show(key, value, timeout=35):
        return app.wait(
            lambda d: value in d["controls"].get(PANE + key, {}).get("text", ""),
            f"{key} to show {value}",
            timeout,
        )

    def page():
        app.click("tabs.plugin_tab_aisle_scene")
        app.click("aisle_scene.asset_mode")
        click("edit_assets")

    def request(description):
        edit("suggest_instruction", description)
        click("suggest_codex" if args.provider == "codex" else "suggest_octos")
        show("asset_feedback", "Proposal ready", 240)
        show("suggest_status", "Pending")

    def choose(delta):
        click("suggest_fields")
        x, y, w, h = app.control(PANE + "suggest_fields")["rect"]
        app.act("click", "w=0", f"x={x + 24}", f"y={y + h / 2 + delta * 30}", "wait=1")

    def point(px, py, kind=None):
        x, y, w, h = app.control(PANE + "photo_canvas.video_area.frame_image")["rect"]
        pos = ("w=0", f"x={x + w * px / 960}", f"y={y + h * py / 640}", "wait=1")
        if kind:
            app.act("mouse", "k=" + kind, *pos)
        else:
            app.act("click", *pos)

    def saved(name, rev):
        return next(
            (p, a)
            for p in (library / "assets").glob(f"*/{rev}.json")
            for a in [json.loads(p.read_bytes())]
            if a["name"] == name
        )

    def snap(name):
        if args.backend == "offscreen":
            app.act("grab", "w=0", "out=" + str(folder / (name + ".png")))

    def sha(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    previous = {p: sha(p) for p in (library / "assets").glob("*/*.json")}
    table_name = "Annotated coffee table"
    with app.launch():
        page()
        show("asset_version", "Coffee table · revision 2")
        click("thumb0.select")
        click("edit_suggestions")
        request(
            "The outlined coffee table in the furnished room. Review its name and collision shape."
        )
        show("suggest_source", "room.jpg")
        app.scroll_to(PANE + "suggest_fields", PANE + "form_scroll")
        snap("survey-proposal")
        # Switch away, then apply a correction. The editor must restore the source photo.
        click("thumb1.select")
        edit("suggest_value", table_name)
        click("suggest_apply")
        show("file_label", "room.jpg")
        show("suggest_status", "Corrected")
        click("save_asset")
        show("asset_version", table_name + " · revision 3")
        _, table = saved(table_name, 3)
        first = table["suggestions"][0]
        assert (
            first["provider"] == args.provider and first.get("flat_reference") is None
        )
        assert {f["field"] for f in first["fields"]} <= {"name", "shape"}
        assert table["name_origin"] == {"source": "operator"}
        original_table = json.loads(
            next(
                p for p in previous if p.parent.name == table["id"] and p.stem == "2"
            ).read_bytes()
        )
        assert (
            table["geometry"] == original_table["geometry"]
            and table["dimensions"] == original_table["dimensions"]
        )
        click("new_asset")
        app.click(PANE + "photo_canvas", "drop", "path=" + str(board))
        show("file_label", board.name)
        edit("asset_name", "Calibration plate")
        for pt in [(240, 160), (720, 160), (720, 480), (240, 480)]:
            point(*pt)
        click("dimension")
        point(240, 535)
        point(480, 535)
        click("edit_suggestions")
        edit("suggest_scale", "0.12")
        request(
            "The outlined blue rectangular calibration plate in a front-on view. Its black coplanar reference is 0.12 metres long."
        )
        click("suggest_reject")
        show("suggest_status", "Discarded")
        click("save_asset")
        show("asset_version", "Calibration plate · revision 1")
        pending_path, plate = saved("Calibration plate", 1)
        pending_hash = sha(pending_path)
        prop = plate["suggestions"][0]
        width_index = next(
            i for i, f in enumerate(prop["fields"]) if f["field"] == "width"
        )
        click("edit_suggestions")
        choose(width_index)
        show("suggest_status", "Pending")
        click("suggest_apply")
        show("suggest_status", "Accepted")
        snap("reviewed-dimension")
        click("suggest_outline")
        point(240, 160, "down")
        point(261, 171, "move")
        point(261, 171, "up")
        click("save_asset")
        show("asset_version", "Calibration plate · revision 2")
    with app.launch():
        page()
        show("asset_version", table_name + " · revision 3")
        click("next_asset")
        show("asset_version", "Calibration plate · revision 2")
        click("edit_suggestions")
        click("suggest_photo")
        show("file_label", board.name)
        choose(width_index)
        show("suggest_status", "Accepted")
    _, plate = saved("Calibration plate", 2)
    final = plate["suggestions"][0]
    assert sha(pending_path) == pending_hash and all(
        sha(p) == h for p, h in previous.items()
    )
    for proposal in [first, final]:
        source = library / "photos" / proposal["photo"]
        meta = json.loads((source / "photo.json").read_bytes())
        assert meta["id"] == proposal["photo"]
        assert (
            sha(source / ("original." + meta["format"]))
            == meta["sha256"]
            == proposal["photo_sha256"]
        )
    assert final["photo_sha256"] == sha(board)
    assert (
        first["input_mode"]
        == final["input_mode"]
        == (
            "survey RGB and region measurements"
            if args.provider == "codex"
            else "RGB-derived region measurements"
        )
    )
    assert (
        final["fields"][0]["status"] == "discarded"
        and final["fields"][width_index]["status"] == "accepted"
    )
    dim = plate["dimensions"][0]
    assert abs(dim["value"] - 0.24) < 0.004 and dim["unit"] == "m", dim
    assert dim["provenance"] == dict(
        source="suggested",
        photo=final["photo"],
        provider=args.provider,
        model=final["model"],
        request_sha256=final["request_sha256"],
    )
    original, new = final["outline"][0], plate["photos"][final["photo"]]["outline"][0]
    assert abs(new[0] - original[0]) > 10 and abs(new[0] - 261) < 5, (new, original)
    result = dict(
        backend=args.backend,
        provider=args.provider,
        unscaled_table=first,
        calibrated_plate=final,
        width_m=dim["value"],
        reference_expected_width_m=0.24,
        edited_outline_point=new,
        restored=True,
        previous_revisions_intact=True,
    )
    (folder / "verification.json").write_text(json.dumps(result, indent=2) + "\n")
    print(
        f"{args.provider}: reviewed both photos and reopened {dim['value']} m width",
        flush=True,
    )


if __name__ == "__main__":
    main()
