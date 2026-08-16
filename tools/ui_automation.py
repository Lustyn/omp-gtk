"""Reusable AT-SPI and screenshot helpers for omp-native desktop UI checks."""

from pathlib import Path
import subprocess
import time

import pyatspi


def descendants(root):
    """Yield root and every currently reachable AT-SPI descendant."""
    stack = [root]
    while stack:
        node = stack.pop()
        yield node
        try:
            children = [node[index] for index in range(node.childCount)]
        except Exception:
            children = []
        stack.extend(reversed(children))


def wait_for(probe, timeout=5.0, interval=0.05):
    """Return the first truthy probe result, or raise after timeout."""
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result:
                return result
        except Exception as error:
            last_error = error
        time.sleep(interval)
    detail = f": {last_error}" if last_error else ""
    raise TimeoutError(f"UI condition was not met within {timeout:.1f}s{detail}")


def application(name="omp-native", timeout=10.0):
    """Wait for and return an AT-SPI application node by exact name."""
    desktop = pyatspi.Registry.getDesktop(0)
    return wait_for(
        lambda: next((app for app in desktop if app.name == name), None),
        timeout=timeout,
    )


def find_node(*, root=None, app_name="omp-native", role=None, name=None, contains=None,
              showing=True, timeout=5.0):
    """Wait for one accessible node matching role and accessible name."""
    search_root = root or application(app_name, timeout=timeout)

    def probe():
        for node in descendants(search_root):
            try:
                node_name = node.name or ""
                if role is not None and node.getRoleName() != role:
                    continue
                if name is not None and node_name != name:
                    continue
                if contains is not None and contains not in node_name:
                    continue
                if showing and not node.getState().contains(pyatspi.STATE_SHOWING):
                    continue
                return node
            except Exception:
                continue
        return None

    return wait_for(probe, timeout=timeout)


def click(node):
    """Invoke a node's accessible click or activation action."""
    actions = node.queryAction()
    for index in range(actions.nActions):
        if actions.getName(index) in {"click", "activate"}:
            if not actions.doAction(index):
                raise RuntimeError(f"AT-SPI activation was rejected for {node.name!r}")
            return
    available = [actions.getName(index) for index in range(actions.nActions)]
    raise RuntimeError(
        f"No click or activate action for {node.getRoleName()} {node.name!r}; actions={available}"
    )


def replace_text(node, text):
    """Replace the contents of an editable entry or GTK text view."""
    if not node.queryEditableText().setTextContents(text):
        raise RuntimeError(f"AT-SPI text replacement was rejected for {node.name!r}")


def visible_names(root=None, app_name="omp-native", role=None):
    """Return accessible names for currently visible nodes."""
    search_root = root or application(app_name)
    result = []
    for node in descendants(search_root):
        try:
            if role is not None and node.getRoleName() != role:
                continue
            if node.name and node.getState().contains(pyatspi.STATE_SHOWING):
                result.append(node.name)
        except Exception:
            continue
    return result


def accessible_tree(root):
    """Return stable role/name/action rows for diagnostics."""
    result = []
    for node in descendants(root):
        try:
            actions = node.queryAction()
            action_names = [actions.getName(index) for index in range(actions.nActions)]
            if node.name or action_names:
                result.append((node.getRoleName(), node.name or "", action_names))
        except Exception:
            continue
    return result


def screenshot_active(filename, directory=Path("artifacts/screenshots")):
    """Capture the active window with Spectacle and return the saved path."""
    directory.mkdir(parents=True, exist_ok=True)
    destination = (directory / filename).resolve()
    subprocess.run(
        [
            "spectacle",
            "--activewindow",
            "--background",
            "--nonotify",
            "--output",
            str(destination),
        ],
        check=True,
    )
    if not destination.is_file():
        raise RuntimeError(f"Spectacle did not create {destination}")
    return destination

# Backward-compatible name used by the documented interactive examples.
screenshot = screenshot_active
