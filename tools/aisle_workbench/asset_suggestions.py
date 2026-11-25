"""Ask the selected provider to propose reviewable properties for one survey photo."""

import argparse
import json
import math
from pathlib import Path
import sys
import tempfile

import cv2
import numpy as np
from PIL import Image, ImageOps

from worker import digest, request_controller

AXES = ("width", "depth", "height")


def regions(rgb, original_size, marks):
    h, w = rgb.shape[:2]
    scale = np.asarray(original_size) / [w, h]
    outline = marks.get("outline", [])
    polygons = []
    if len(outline) >= 3:
        polygons.append(("operator-outline", outline))
    edges = cv2.Canny(cv2.GaussianBlur(rgb, (5, 5), 0), 35, 100)
    edges = cv2.morphologyEx(edges, cv2.MORPH_CLOSE, np.ones((5, 5), np.uint8))
    contours, _ = cv2.findContours(edges, cv2.RETR_LIST, cv2.CHAIN_APPROX_SIMPLE)
    for contour in sorted(contours, key=cv2.contourArea, reverse=True):
        if len(polygons) >= 9:
            break
        if not 0.006 * w * h <= cv2.contourArea(contour) <= 0.95 * w * h:
            continue
        poly = cv2.approxPolyDP(
            contour, 0.018 * cv2.arcLength(contour, True), True
        ).reshape(-1, 2)
        if 3 <= len(poly) <= 32:
            polygons.append(
                (f"image-region-{len(polygons) + 1}", (poly * scale).round(2).tolist())
            )
    result = []
    for name, points in polygons:
        p = np.asarray(points, dtype=float)
        if (
            p.ndim != 2
            or p.shape[1] != 2
            or not np.isfinite(p).all()
            or np.any(p < 0)
            or np.any(p >= original_size)
        ):
            raise ValueError("A photo outline contains invalid source pixels")
        mask = np.zeros((h, w), np.uint8)
        cv2.fillPoly(mask, [np.rint(p / scale).astype(np.int32)], 255)
        mean = cv2.mean(rgb, mask=mask)[:3]
        result.append(
            dict(
                id=name,
                outline=p.tolist(),
                bounds_px=[p.min(axis=0).tolist(), p.max(axis=0).tolist()],
                mean_rgb=[round(v) for v in mean],
            )
        )
    return result


def planar_scale(sheet, size):
    reference = sheet.get("flat_reference")
    if reference is None:
        return None
    axis = reference["axis"]
    if type(axis) is not int or axis not in range(3):
        raise ValueError("Choose width, depth or height for the planar reference")
    line = np.asarray(sheet["marks"]["rulers"][axis], dtype=float)
    metres = float(reference["length_m"])
    if (
        line.shape != (2, 2)
        or not np.isfinite(line).all()
        or np.any(line < 0)
        or np.any(line >= size)
        or not np.array_equal(line, reference["endpoints"])
        or not math.isfinite(metres)
        or metres <= 0
    ):
        raise ValueError(
            "The calibration must match the marked reference in the source photo"
        )
    direction = line[1] - line[0]
    length = np.linalg.norm(direction)
    if length < 2:
        raise ValueError("The reference endpoints are too close together")
    return AXES[axis], direction / length, metres / length


def suggest(photo_dir, sheet, provider, work):
    photo = json.loads((photo_dir / "photo.json").read_bytes())
    source = photo_dir / ("original." + photo["format"])
    if digest(source) != photo["sha256"] or photo["id"] != sheet["photo"]:
        raise ValueError("The selected source photo has changed")
    with Image.open(source) as original:
        oriented = ImageOps.exif_transpose(original).convert("RGB")
    size = oriented.size
    if size != (photo["width"], photo["height"]):
        raise ValueError("Photo dimensions do not match the saved annotation frame")
    scale = planar_scale(sheet, size)
    oriented.thumbnail((1000, 850))
    candidates = regions(np.asarray(oriented), size, sheet["marks"])
    if not candidates:
        raise ValueError("Mark an outline around the target and try again")
    description = sheet["description"].strip()
    schema = dict(
        type="object",
        additionalProperties=False,
        properties={
            "supported": {"type": "boolean"},
            "region": {"type": "string", "enum": [c["id"] for c in candidates]},
            "name": {"type": "string"},
            "shape": {"type": "string", "enum": ["box", "four_leg_table", "unknown"]},
            "reason": {"type": "string"},
        },
        required=["supported", "region", "name", "shape", "reason"],
    )
    observations = dict(
        original_size_px=list(size),
        regions=candidates,
        operator_description=description,
        asset_name=sheet.get("name", ""),
        flat_reference=sheet.get("flat_reference"),
    )
    prompt = (
        "Review one physical asset for a robotics survey. Select the region best matching the operator description. "
        "An operator-outline, when present, identifies the object. If the identity is ambiguous set supported=false. "
        "Propose a concise name and a collision shape (box, four_leg_table or unknown); explain the uncertainty. "
        "Dimensions are computed separately from marked rulers. Photo measurements use oriented source pixels.\n"
        + json.dumps(observations)
    )
    attachment = None
    if provider == "codex":
        image = work / "survey.png"
        oriented.save(image)
        attachment = dict(
            images=[dict(file=image.name, sha256=digest(image))],
            photo_sha256=photo["sha256"],
        )
    choice, model = request_controller(
        provider, prompt, schema, work, survey_input=attachment
    )
    if choice.get("supported") is not True:
        raise ValueError(
            str(choice.get("reason") or "The model could not identify the object")[:300]
        )
    chosen = next((r for r in candidates if r["id"] == choice.get("region")), None)
    if (
        chosen is None
        or not isinstance(choice.get("name"), str)
        or not 1 <= len(choice["name"].strip()) <= 100
    ):
        raise ValueError("The model did not return a valid photo region and asset name")
    fields = [dict(field="name", value=choice["name"].strip())]
    if choice["shape"] in ("box", "four_leg_table"):
        fields.append(dict(field="shape", value=choice["shape"]))
    elif choice["shape"] != "unknown":
        raise ValueError("Unsupported proposed collision shape")
    # Only use the operator's explicit coplanar reference for this projection.
    # An ordinary object ruler on a perspective photo does not calibrate its silhouette.
    reference = sheet.get("flat_reference")
    if scale is not None:
        axis, direction, metres_per_px = scale
        estimate = np.ptp(np.array(chosen["outline"]) @ direction) * metres_per_px
        if math.isfinite(estimate) and estimate > 0:
            fields.append(dict(field=axis, value=format(estimate, ".6g")))
    return dict(
        photo=photo["id"],
        photo_sha256=photo["sha256"],
        provider=provider,
        model=model,
        flat_reference=reference,
        input_mode=(
            "survey RGB and region measurements"
            if attachment
            else "RGB-derived region measurements"
        ),
        request_sha256=digest(work / "controller-request.json"),
        outline=chosen["outline"],
        reason=str(choice.get("reason", ""))[:350],
        fields=fields,
    )


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("photo_dir", type=Path)
    cli.add_argument("provider", choices=["codex", "octos-deepseek"])
    args = cli.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        print(
            json.dumps(
                suggest(args.photo_dir, json.load(sys.stdin), args.provider, Path(tmp))
            )
        )


if __name__ == "__main__":
    main()
