# UI automation and screenshots

This project can be smoke-tested through Linux AT-SPI. GTK exposes the live widget tree over AT-SPI; Python can inspect that tree, invoke accessible actions, edit text, and assert visible state without depending on screen coordinates. Spectacle captures the active application window on the KDE development workstation.

This workflow is intended for interactive verification and screenshot capture. Keep permanent tests in Rust when the behavior does not require a live desktop.

## Prerequisites

Run the automation from a graphical Linux session with the accessibility bus available.

```bash
sudo apt install python3-gi gir1.2-atspi-2.0 spectacle python3-pil
/usr/bin/python3 -c 'import pyatspi; print("AT-SPI ready")'
cargo build
```

Launch the application from the repository root:

```bash
target/debug/omp-native
```

`omp-native` starts `omp --mode rpc-ui`. Set `OMP_BIN` only when a specific installed OMP executable must be exercised:

```bash
OMP_BIN=/path/to/omp target/debug/omp-native
```

Close any existing `omp-native` windows before launching a separate test process. `dev.omp.Native` is a single-instance GTK application, so a second invocation can activate the existing process and exit immediately.

## Python helper functions

Start `/usr/bin/python3` or an IPython kernel and load these helpers once. A persistent kernel can reuse them in later cells.

```python
from pathlib import Path
import subprocess
import time

import pyatspi

APP_NAME = "omp-native"


def descendants(root):
    """Yield root and every currently reachable AT-SPI descendant."""
    stack = [root]
    while stack:
        node = stack.pop()
        yield node
        try:
            children = [node[index] for index in range(node.childCount)]
        except Exception:
            # The GTK tree can change while a dialog opens or a row disappears.
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


def application(timeout=10.0):
    """Wait for and return the omp-native AT-SPI application node."""
    desktop = pyatspi.Registry.getDesktop(0)
    return wait_for(
        lambda: next((app for app in desktop if app.name == APP_NAME), None),
        timeout=timeout,
    )


def find_node(*, root=None, role=None, name=None, contains=None, showing=True, timeout=5.0):
    """Wait for one accessible node matching role and accessible name."""
    search_root = root or application(timeout=timeout)

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
                # A stale node is normal while GTK replaces a popover or list.
                continue
        return None

    return wait_for(probe, timeout=timeout)


def click(node):
    """Invoke a node's accessible click action."""
    actions = node.queryAction()
    for index in range(actions.nActions):
        if actions.getName(index) == "click":
            if not actions.doAction(index):
                raise RuntimeError(f"AT-SPI click was rejected for {node.name!r}")
            return
    available = [actions.getName(index) for index in range(actions.nActions)]
    raise RuntimeError(f"No click action for {node.getRoleName()} {node.name!r}; actions={available}")


def replace_text(node, text):
    """Replace the contents of an editable entry or GTK text view."""
    if not node.queryEditableText().setTextContents(text):
        raise RuntimeError(f"AT-SPI text replacement was rejected for {node.name!r}")


def visible_names(root=None, role=None):
    """Return accessible names for currently visible nodes, useful for assertions."""
    search_root = root or application()
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


def screenshot(filename, directory=Path("artifacts/screenshots")):
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
```

Run Python from the repository root if screenshots should be written beneath `artifacts/screenshots/`. Generated screenshots are verification artifacts; add only intentional reference images to Git.

## Inspect the accessible tree

Accessible names can include text from a button's child widgets. Dump roles, names, and actions before choosing a selector:

```python
app = application()
for node in descendants(app):
    try:
        actions = node.queryAction()
        action_names = [actions.getName(index) for index in range(actions.nActions)]
        if node.name or action_names:
            print(node.getRoleName(), repr(node.name), action_names)
    except Exception:
        pass
```

Prefer semantic selectors such as role plus accessible name. Do not select by child index unless the order itself is the behavior under test.

## Example: open and filter the model picker

The model button's accessible name contains its provider and current model. The picker dialog and filter chips expose stable semantic names.

```python
app = application()

model_button = find_node(root=app, role="button", contains="GPT-5.6")
click(model_button)

dialog = find_node(root=app, role="dialog", name="Choose a model")
search = find_node(root=dialog, role="entry", name="")
replace_text(search, "claude")

models = visible_names(dialog, role="button")
assert any("Claude Opus" in value for value in models)
assert not any("Gemini" in value for value in models)

path = screenshot("model-picker-claude.png")
print(path)
```

To exercise provider or context-size filters:

```python
click(find_node(root=dialog, role="toggle button", name="Anthropic"))
assert any("Claude Opus" in value for value in visible_names(dialog, role="button"))

click(find_node(root=dialog, role="toggle button", name="All providers"))
click(find_node(root=dialog, role="toggle button", name="257K+"))
large_models = visible_names(dialog, role="button")
assert any("Gemini" in value for value in large_models)
```

Model result rows are buttons. Select one through its accessible action rather than synthesizing a pointer click:

```python
row = find_node(root=dialog, role="button", contains="Gemini 3 Pro")
click(row)
wait_for(lambda: any("Gemini 3 Pro" in name for name in visible_names(app, role="button")))
```

## Example: edit and submit the composer

GTK exposes the multiline composer as a `text` role. Use `queryEditableText()` for input and the accessible action for submission.

```python
app = application()
composer = find_node(root=app, role="text", name="")
replace_text(composer, "Summarize the current project structure")

send = find_node(root=app, role="button", contains="Send")
click(send)

wait_for(
    lambda: any(
        "Summarize the current project structure" in name
        for name in visible_names(app, role="label")
    ),
    timeout=10.0,
)
```

Only submit prompts when exercising the real OMP session is intentional. Text entry by itself is safe for layout screenshots; clear it with `replace_text(composer, "")` afterward.

## Example: verify titles loaded from disk

Session titles are labels in the conversation list. The active title is also exposed as a label in the header.

```python
app = application()
labels = visible_names(app, role="label")
assert "Improve native app hero text" in labels
```

For a title written while the app is open, poll the accessible labels. The app watches session files and refreshes titles asynchronously:

```python
wait_for(
    lambda: "Expected generated title" in visible_names(app, role="label"),
    timeout=5.0,
)
```

Do not rewrite real session JSONL files for automation. Exercise an existing titled session or use a temporary directory owned by the test process.

## Screenshot checks

Capture at least two states for animated UI when movement matters. Pillow can confirm that pixels changed while the image dimensions remained stable:

```python
from PIL import Image, ImageChops

a_path = screenshot("status-a.png")
time.sleep(0.2)
b_path = screenshot("status-b.png")

a = Image.open(a_path).convert("RGB")
b = Image.open(b_path).convert("RGB")
assert a.size == b.size
assert ImageChops.difference(a, b).getbbox() is not None
```

A full-window difference can include cursor, compositor, or unrelated animation. Crop to the component under test when exact evidence is required:

```python
box = (995, 10, 1085, 42)  # left, top, right, bottom for the captured window
changed = ImageChops.difference(a.crop(box), b.crop(box))
assert changed.getbbox() is not None
```

Coordinates are acceptable for image analysis after capture. Avoid coordinate-driven interaction: under Wayland, AT-SPI component extents can temporarily report `(0, 0)`, while accessible actions remain reliable.

## Reliability rules

- Wait for observable state; do not use fixed sleeps as the only readiness check.
- Reacquire nodes after opening or closing dialogs. GTK can destroy and replace accessible objects.
- Prefer `queryAction().doAction()` and `queryEditableText()` over mouse and keyboard synthesis.
- Scope searches to the dialog or container being tested to avoid duplicate names.
- Assert behavior before capturing a screenshot. A screenshot alone does not prove the interaction succeeded.
- Keep the application focused before `spectacle --activewindow`; otherwise Spectacle captures a different window.
- Clean up generated screenshots and terminate test-launched processes after verification.
