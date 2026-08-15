#!/usr/bin/python3
"""Build, inspect, exercise, and capture native GTK component stories."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from ui_automation import application, find_node, screenshot_active


ROOT = Path(__file__).resolve().parents[1]
GALLERY = ROOT / "target/debug/ui-gallery"
APP_NAME = "ui-gallery"
VIRTUAL_SESSION = ROOT / "tools/ui_story_inspect_session.py"


def build_gallery():
    subprocess.run(
        ["cargo", "build", "--features", "ui-stories", "--bin", "ui-gallery"],
        cwd=ROOT,
        check=True,
    )


def list_stories():
    build_gallery()
    subprocess.run([str(GALLERY), "--list"], cwd=ROOT, check=True)


def launch_story(story_id):
    build_gallery()
    process = subprocess.Popen(
        [str(GALLERY), "--story", story_id],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        app = application(APP_NAME, timeout=10.0)
        find_node(
            root=app,
            role="label",
            name=f"Story canvas: {story_id}",
            timeout=10.0,
        )
        return process, app
    except Exception:
        stop(process)
        stderr = process.stderr.read() if process.stderr else ""
        if stderr:
            print(stderr, file=sys.stderr)
        raise


def stop(process):
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5.0)


def run_virtual_story(story_id, mode, steps=None, output=None, expect=None):
    build_gallery()
    environment = os.environ.copy()
    environment.update(
        {
            "OMP_UI_STORY_ID": story_id,
            "OMP_UI_STORY_MODE": mode,
        }
    )
    if steps is not None:
        environment["OMP_UI_STORY_STEPS"] = json.dumps(steps)
    if output is not None:
        environment["OMP_UI_STORY_OUTPUT"] = str(Path(output).resolve())
    if expect is not None:
        environment["OMP_UI_STORY_EXPECT"] = expect
    with tempfile.TemporaryDirectory(prefix="omp-ui-story-") as runtime_dir:
        os.chmod(runtime_dir, 0o700)
        environment["XDG_RUNTIME_DIR"] = runtime_dir
        result = subprocess.run(
            [
                "dbus-run-session",
                "--",
                "kwin_wayland",
                "--virtual",
                "--width",
                "1200",
                "--height",
                "900",
                "--no-lockscreen",
                "--no-global-shortcuts",
                "--no-kactivities",
                "--exit-with-session",
                str(VIRTUAL_SESSION),
            ],
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
        )
    if result.stdout:
        print(result.stdout, end="")
    if result.returncode:
        if result.stderr:
            print(result.stderr, file=sys.stderr, end="")
        raise subprocess.CalledProcessError(result.returncode, result.args)


def inspect_story(story_id):
    run_virtual_story(story_id, "inspect")


def exercise_story(story_id, serialized_steps):
    steps = json.loads(serialized_steps)
    if not isinstance(steps, list) or not steps:
        raise ValueError("--steps must be a non-empty JSON array")
    run_virtual_story(story_id, "exercise", steps)


def capture_story(story_id, output, visible=False, window=False, expect=None):
    output = Path(output).resolve()
    if expect is not None and not window:
        raise ValueError("--expect requires --window")
    if window:
        run_virtual_story(story_id, "capture", output=output, expect=expect)
        if not output.is_file():
            raise RuntimeError(f"Virtual compositor did not create {output}")
        print(output)
        return
    if visible:
        process, _app = launch_story(story_id)
        try:
            path = screenshot_active(output.name, output.parent)
            print(path)
        finally:
            stop(process)
        return

    build_gallery()
    daemon, display = start_broadway()
    environment = os.environ.copy()
    environment.update(
        {
            "GDK_BACKEND": "broadway",
            "BROADWAY_DISPLAY": display,
        }
    )
    try:
        subprocess.run(
            [
                str(GALLERY),
                "--story",
                story_id,
                "--snapshot",
                str(output),
            ],
            cwd=ROOT,
            env=environment,
            check=True,
        )
    finally:
        stop(daemon)
    if not output.is_file():
        raise RuntimeError(f"Gallery did not create {output}")
    print(output)


def start_broadway():
    for number in range(90, 120):
        display = f":{number}"
        daemon = subprocess.Popen(
            ["gtk4-broadwayd", display],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        time.sleep(0.08)
        if daemon.poll() is None:
            return daemon, display
    raise RuntimeError("Could not start an isolated GTK Broadway display")


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    subcommands = result.add_subparsers(dest="command", required=True)
    subcommands.add_parser("list", help="List available story ids")
    inspect = subcommands.add_parser("inspect", help="Print the accessible tree for a story")
    inspect.add_argument("story_id")
    exercise = subcommands.add_parser(
        "exercise", help="Run semantic interaction steps in an isolated virtual session"
    )
    exercise.add_argument("story_id")
    exercise.add_argument(
        "--steps",
        required=True,
        help="JSON array of click, replace, expect, and expect_absent steps",
    )
    capture = subcommands.add_parser("capture", help="Capture one story window")
    capture.add_argument("story_id")
    capture.add_argument("--output", required=True)
    surface = capture.add_mutually_exclusive_group()
    surface.add_argument(
        "--window",
        action="store_true",
        help="Capture the full window in an isolated virtual Wayland session",
    )
    surface.add_argument(
        "--visible",
        action="store_true",
        help="Capture the active desktop window instead of running headlessly",
    )
    capture.add_argument(
        "--expect",
        help="Wait for accessible text before taking a virtual window capture",
    )
    return result


def main():
    arguments = parser().parse_args()
    if arguments.command == "list":
        list_stories()
    elif arguments.command == "inspect":
        inspect_story(arguments.story_id)
    elif arguments.command == "exercise":
        exercise_story(arguments.story_id, arguments.steps)
    elif arguments.command == "capture":
        capture_story(
            arguments.story_id,
            arguments.output,
            visible=arguments.visible,
            window=arguments.window,
            expect=arguments.expect,
        )


if __name__ == "__main__":
    main()
