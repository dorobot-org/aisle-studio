"""Check URDF materials and compare task-object visibility in all three sensors."""

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np

from nano_living import CAMERAS
from nano_visuals import visual_specs


def visibility(run):
    info = json.loads((run / "scene-info.json").read_text())
    count = json.loads((run / "result.json").read_text())["frames"]
    task_entities = {
        i
        for i, name in enumerate(info["segmentation_entities"])
        if name.startswith(("sofa_", "cube_", "table_leg_")) or name == "coffee_table"
    }
    ids = [
        int(k)
        for k, v in info["segmentation_id_map"].items()
        if isinstance(v, list) and v[0] in task_entities
    ]
    fractions = {name: [] for name in CAMERAS}
    for i in sorted(set(np.linspace(0, count - 1, 40, dtype=int).tolist())):
        with np.load(run / f"raw/frame_{i:06d}.npz", allow_pickle=False) as source:
            for name in CAMERAS:
                fractions[name].append(
                    float(np.isin(source[f"{name}__seg"], ids).mean())
                )
    return {
        name: dict(
            mean_task_pixel_fraction=float(np.mean(values)),
            samples_with_visible_task_objects=sum(v > 0.001 for v in values),
            samples=len(values),
        )
        for name, values in fractions.items()
    }


def verify(run, baseline):
    info = json.loads((run / "scene-info.json").read_text())
    expected = visual_specs(info["model"])
    actual = info["robot_visuals"]
    assert len(expected) == len(actual) == 28
    for a, e in zip(actual, expected):
        assert a["link"] == e["link"] and a["rgba"] == e["rgba"]
        assert hashlib.sha256(Path(a["path"]).read_bytes()).hexdigest() == a["sha256"]
        assert a["triangles"] > 0
    old = json.loads((baseline / "scene-info.json").read_text())
    for name in CAMERAS:
        assert info["cameras"][name] == old["cameras"][name], (
            "Optical mounts must remain unchanged"
        )
    before, after = visibility(baseline), visibility(run)
    for name in CAMERAS:
        assert (
            after[name]["mean_task_pixel_fraction"]
            > before[name]["mean_task_pixel_fraction"]
        ), (name, before[name], after[name])
    report = dict(
        ok=True,
        visual_meshes=28,
        named_materials=True,
        mesh_hashes=True,
        camera_mounts_unchanged=True,
        before=before,
        after=after,
        note="Visibility uses simulator segmentation for sofa/table/cubes, not aesthetic scoring or hardware calibration.",
    )
    (run / "visual-verification.json").write_text(json.dumps(report, indent=2))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("baseline", type=Path)
    args = parser.parse_args()
    print(json.dumps(verify(args.run, args.baseline)))
