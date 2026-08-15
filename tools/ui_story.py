#!/usr/bin/python3
"""Build, inspect, and capture native GTK component stories."""

import argparse
import os
from pathlib import Path
import subprocess
import sys
import time

from ui_automation import accessible_tree, application, find_node, screenshot_active


ROOT = Path(__file__).resolve().parents[1]
GALLERY = ROOT / "target/debug/ui-gallery"
APP_NAME = "ui-gallery"


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


def inspect_story(story_id):
    process, app = launch_story(story_id)
    try:
        for role, name, actions in accessible_tree(app):
            print(f"{role}: {name!r} {actions}")
    finally:
        stop(process)


def capture_story(story_id, output, visible=False):
    output = Path(output).resolve()
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
    capture = subcommands.add_parser("capture", help="Capture one story window")
    capture.add_argument("story_id")
    capture.add_argument("--output", required=True)
    capture.add_argument(
        "--visible",
        action="store_true",
        help="Capture the active desktop window instead of using headless Broadway rendering",
    )
    return result


def main():
    arguments = parser().parse_args()
    if arguments.command == "list":
        list_stories()
    elif arguments.command == "inspect":
        inspect_story(arguments.story_id)
    elif arguments.command == "capture":
        capture_story(arguments.story_id, arguments.output, visible=arguments.visible)


if __name__ == "__main__":
    main()
