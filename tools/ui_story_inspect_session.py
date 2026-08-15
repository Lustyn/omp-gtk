#!/usr/bin/python3
"""Inspect, exercise, or capture one GTK story in virtual Wayland."""

import json
import os
from pathlib import Path
import subprocess
import sys

from ui_automation import (
    accessible_tree,
    application,
    click,
    find_node,
    replace_text,
    visible_names,
    wait_for,
)


ROOT = Path(__file__).resolve().parents[1]
GALLERY = ROOT / "target/debug/ui-gallery"
APP_NAME = "ui-gallery"


def stop(process):
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5.0)


def matches_step(app, step):
    names = visible_names(app, role=step.get("role"))
    expected = step.get("name")
    contains = step.get("contains")
    return any(
        (expected is None or value == expected)
        and (contains is None or contains in value)
        for value in names
    )


def exercise(app, steps):
    for step_number, step in enumerate(steps, start=1):
        action = step.get("action")
        if action in {"click", "replace"}:
            node = find_node(
                root=app,
                role=step.get("role"),
                name=step.get("name"),
                contains=step.get("contains"),
            )
            if action == "click":
                click(node)
            else:
                replace_text(node, step["text"])
        elif action == "expect":
            wait_for(lambda: matches_step(app, step))
        elif action == "expect_absent":
            wait_for(lambda: not matches_step(app, step))
        else:
            raise ValueError(f"Unsupported action in step {step_number}: {action!r}")
        print(f"UI_STORY_STEP {step_number} {action} PASS")


def inspect(app):
    for role, name, actions in accessible_tree(app):
        print(f"{role}: {name!r} {actions}")


def screenshot_virtual_screen(output):
    output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "spectacle",
            "--fullscreen",
            "--background",
            "--nonotify",
            "--output",
            str(output),
        ],
        check=True,
    )
    if not output.is_file():
        raise RuntimeError(f"Spectacle did not create {output}")
    return output


def main():
    story_id = os.environ.get("OMP_UI_STORY_ID")
    if not story_id:
        print("OMP_UI_STORY_ID is required", file=sys.stderr)
        return 2

    environment = os.environ.copy()
    environment["GTK_A11Y"] = "atspi"
    process = subprocess.Popen(
        [str(GALLERY), "--story", story_id],
        cwd=ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        app = application(APP_NAME, timeout=15.0)
        find_node(
            root=app,
            role="label",
            name=f"Story canvas: {story_id}",
            timeout=15.0,
        )
        mode = os.environ.get("OMP_UI_STORY_MODE", "inspect")
        if mode == "inspect":
            inspect(app)
        elif mode == "exercise":
            steps = json.loads(os.environ["OMP_UI_STORY_STEPS"])
            exercise(app, steps)
        elif mode == "capture":
            if expected := os.environ.get("OMP_UI_STORY_EXPECT"):
                find_node(root=app, contains=expected, timeout=15.0)
            output = Path(os.environ["OMP_UI_STORY_OUTPUT"])
            path = screenshot_virtual_screen(output)
            print(f"UI_STORY_WINDOW_SNAPSHOT {path}")
        else:
            raise ValueError(f"Unsupported story mode: {mode!r}")
    except Exception:
        stderr = process.stderr.read() if process.poll() is not None and process.stderr else ""
        if stderr:
            print(stderr, file=sys.stderr)
        raise
    finally:
        stop(process)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
