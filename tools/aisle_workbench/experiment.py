"""Freeze a configuration and realize a reproducible set of Studio trials."""

import copy
import hashlib
import json
import math
import random
from uuid import UUID, uuid4

MAX_SEED = 2**32 - 1


def sha(data):
    return hashlib.sha256(data).hexdigest()


def encode(value):
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode()


def finite(value, label):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ValueError(f"{label} must be finite")
    return value


def seeds(config):
    n = config["repeat_count"]
    if type(n) is not int or not 1 <= n <= 10000:
        raise ValueError("Repeat count must be between 1 and 10000")
    mode = config["seed_mode"]
    if mode == "explicit":
        values = config["seeds"]
        if len(values) != n:
            raise ValueError("Enter one seed per repetition")
    else:
        base = config["base_seed"]
        if type(base) is not int or not 0 <= base <= MAX_SEED:
            raise ValueError("A seed must fit an unsigned 32-bit integer")
        if mode == "identical":
            values = [base] * n
        elif mode == "sequence":
            values = [(base + i) & MAX_SEED for i in range(n)]
        else:
            raise ValueError("Choose identical, sequential, or explicit seeds")
    if any(type(v) is not int or not 0 <= v <= MAX_SEED for v in values):
        raise ValueError("Invalid seed")
    return values


def draw(parameter, seed):
    kind = parameter["type"]
    name = parameter["name"]
    # Per-parameter streams keep samples stable when the operator adds an unrelated field.
    rng = random.Random(int(sha(encode([seed, name])), 16))
    value = parameter["default"]
    if kind in ("number", "integer"):
        finite(value, name)
        if kind == "integer" and type(value) is not int:
            raise ValueError(f"{name} needs an integer")
        bounds = parameter.get("range")
        if bounds is not None:
            lo, hi = bounds
            finite(lo, name)
            finite(hi, name)
            if not lo <= value <= hi:
                raise ValueError(f"{name} default is outside its range")
            if kind == "integer":
                if type(lo) is not int or type(hi) is not int:
                    raise ValueError(f"{name} needs integer range endpoints")
                value = rng.randint(lo, hi)
            else:
                value = rng.uniform(lo, hi)
    elif kind == "boolean":
        if type(value) is not bool:
            raise ValueError(f"{name} needs a boolean")
        if parameter.get("randomize"):
            value = bool(rng.getrandbits(1))
    elif kind == "enum":
        choices = parameter["allowed_values"]
        if (
            not isinstance(value, str)
            or not all(isinstance(v, str) for v in choices)
            or value not in choices
        ):
            raise ValueError(f"{name} needs an allowed value")
        if parameter.get("randomize"):
            value = rng.choice(choices)
    else:
        raise ValueError(f"Unknown parameter type for {name}")
    return value


def sample(task, config, seed):
    """The bound task stays immutable. Each realized execution gets its own layout and overrides."""
    effective = copy.deepcopy(task)
    items = {i["id"]: i for i in effective["scene"]["instances"]}
    values = {}
    occupied = set()
    environment = {}
    materials = {}
    controller = {}

    def claim(key):
        if key in occupied:
            raise ValueError(f"Two parameters write {key}")
        occupied.add(key)

    def object_(identifier):
        if identifier not in items:
            raise ValueError(f"Missing bound instance {identifier}")
        return items[identifier]

    for p in config.get("parameters", []):
        name = p["name"].strip()
        if not name or name in values:
            raise ValueError("Parameters need unique names")
        value = draw(p, seed)
        binding = p.get("binding") or {}
        kind = binding.get("kind")
        unit = p.get("unit", "")
        if kind in ("position", "spacing"):
            if p["type"] not in ("number", "integer") or unit != "m":
                raise ValueError(f"{name} must be a distance in metres")
            axis = {"x": 0, "y": 1, "z": 2}.get(binding.get("axis"))
            if axis is None:
                raise ValueError(f"{name} needs an X, Y or Z binding")
            if kind == "position":
                obj = object_(binding["instance"])
                claim((obj["id"], axis))
                obj["position_m"][axis] = value
            else:
                first, second = map(object_, binding["instances"])
                if first["id"] == second["id"] or value <= 0:
                    raise ValueError(
                        f"{name} requires two distinct objects and positive spacing"
                    )
                claim((first["id"], axis))
                claim((second["id"], axis))
                midpoint = (first["position_m"][axis] + second["position_m"][axis]) / 2
                direction = (
                    1 if first["position_m"][axis] <= second["position_m"][axis] else -1
                )
                first["position_m"][axis] = midpoint - direction * value / 2
                second["position_m"][axis] = midpoint + direction * value / 2
        elif kind == "yaw":
            if p["type"] not in ("number", "integer") or unit != "deg":
                raise ValueError(f"{name} needs degrees")
            obj = object_(binding["instance"])
            claim((obj["id"], "yaw"))
            obj["yaw_deg"] = value
        elif kind == "friction":
            if (
                p["type"] not in ("number", "integer")
                or value < 0
                or unit not in ("", "1")
            ):
                raise ValueError(f"{name} needs nonnegative dimensionless friction")
            obj = object_(binding["instance"])
            claim((obj["id"], "friction"))
            materials.setdefault(obj["id"], {})["friction"] = value
        elif kind == "ambient":
            if (
                p["type"] not in ("number", "integer")
                or not 0 <= value <= 1
                or unit not in ("", "1")
            ):
                raise ValueError(f"{name} needs illumination in [0, 1]")
            claim("ambient_rgb")
            environment["ambient_rgb"] = [value] * 3
        elif kind == "wrist_clearance":
            if p["type"] != "boolean":
                raise ValueError(f"{name} needs a boolean")
            claim(kind)
            controller[kind] = value
        elif kind == "visual_profile":
            if p["type"] != "enum" or value not in ("original", "compact-v1"):
                raise ValueError(f"{name} needs a supported camera profile")
            claim(kind)
            controller[kind] = value
        else:
            raise ValueError(f"Parameter {name} is missing a runtime binding")
        values[name] = dict(value=value, type=p["type"], unit=unit, binding=binding)
    return dict(
        seed=seed,
        scene=effective["scene"],
        materials=materials,
        environment=environment,
        controller=controller,
        values=values,
    )


def recordings(config, index):
    row = dict(config["recording"])
    overrides = config.get("overrides", {})
    if any(
        str(int(k)) != k or not 1 <= int(k) <= config["repeat_count"] for k in overrides
    ):
        raise ValueError("Recording override names a missing repetition")
    row.update(overrides.get(str(index), {}))
    if set(row) != {"robot_trace", "agent_history"} or any(
        type(v) is not bool for v in row.values()
    ):
        raise ValueError("Choose each recording channel independently")
    return row


def schedule(config, task, task_hash, controller_hash, robot_hash):
    if not config["name"].strip():
        raise ValueError("Name the experiment")
    batch_id = str(uuid4())
    rows = []
    for i, seed in enumerate(seeds(config), 1):
        record = recordings(config, i)
        rows.append(
            dict(
                run_id=str(uuid4()),
                batch_id=batch_id,
                repetition=i,
                scene=dict(id=task["scene"]["id"], revision=task["scene"]["revision"]),
                task_sha256=task_hash,
                controller_sha256=controller_hash,
                robot=task["robot"],
                robot_model_sha256=robot_hash,
                seed=seed,
                recording=record,
                status="planned",
                parent_run_id=None,
            )
        )
    return dict(
        schema="aisle-experiment-v1",
        batch_id=batch_id,
        configuration=copy.deepcopy(config),
        repetitions=rows,
    )
