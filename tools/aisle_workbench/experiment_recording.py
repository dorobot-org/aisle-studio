"""Optional run channels. Keep retained model exchanges separate from execution measurements."""

import json
from datetime import datetime, timezone
import re


def now():
    return datetime.now(timezone.utc).isoformat()


def line(stream, event):
    stream.write(json.dumps(event, separators=(",", ":"), allow_nan=False) + "\n")


def redact(value):
    if isinstance(value, dict):
        return {
            key: redact(v)
            for key, v in value.items()
            if key.lower()
            not in {"api_key", "authorization", "password", "access_token", "secret"}
        }
    if isinstance(value, list):
        return [redact(v) for v in value]
    if isinstance(value, str):
        value = re.sub(r"(?i)\bBearer\s+[\w.+/=:-]+", "Bearer [redacted]", value)
        return re.sub(
            r"\b(?:sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9_]{20,})\b",
            "[redacted]",
            value,
        )
    return value


def history(source, target, manifest, plan):
    base = dict(
        run_id=manifest["run_id"],
        controller_sha256=manifest["controller_sha256"],
        provider=plan.get("provider"),
        model=plan.get("policy_model"),
    )

    def event(stream, stage, kind, data, path=None):
        when = (
            datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).isoformat()
            if path
            else now()
        )
        line(
            stream,
            base | dict(timestamp=when, stage=stage, event=kind, data=redact(data)),
        )

    with target.open("x") as stream:
        req = source / "controller-request.json"
        if req.is_file():
            data = json.loads(req.read_bytes())
            if "onboard_input" in data:
                event(
                    stream,
                    "authoring",
                    "tool_result",
                    {"tool": "capture_onboard", "result": data["onboard_input"]},
                    req,
                )
            event(
                stream,
                "authoring",
                "prompt",
                {
                    "text": data["instruction"],
                    "images": data.get("onboard_input", {}).get("images", []),
                },
                req,
            )
            rsp = source / (
                "provider-response.json"
                if plan.get("provider") == "octos-deepseek"
                else "reply.json"
            )
            if rsp.is_file():
                event(
                    stream, "authoring", "response", json.loads(rsp.read_bytes()), rsp
                )
            else:
                event(
                    stream,
                    "authoring",
                    "partial_history",
                    {"reason": "Original response artifact unavailable"},
                )
        else:
            event(
                stream,
                "authoring",
                (
                    "reference_controller"
                    if plan.get("provider") == "reference"
                    else "partial_history"
                ),
                {
                    "reason": (
                        "Reference controller used without a model call"
                        if plan.get("provider") == "reference"
                        else "Original prompt and response unavailable"
                    )
                },
            )
        event(
            stream,
            "execution",
            "controller_reused",
            {
                "new_model_calls": 0,
                "source": "prepared controller",
                "runtime": "onboard visual policy",
            },
        )
