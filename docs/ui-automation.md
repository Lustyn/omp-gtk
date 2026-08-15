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

## Component gallery

Use the native component gallery when work does not require a live OMP session. Stories render the production GTK components, icons, and stylesheet with deterministic fixture data.

```bash
cargo build --features ui-stories --bin ui-gallery
target/debug/ui-gallery --list
target/debug/ui-gallery --story composer/running
```

The gallery uses the separate `dev.omp.Native.UiGallery` application ID and does not start `omp --mode rpc-ui`. Direct stories print `UI_STORY_READY <story-id>` after their window is mapped. `--snapshot <path>` renders the component itself through GSK and exits; the story canvas chrome is not included.

The automation wrapper builds the gallery and can inspect a visible story through AT-SPI:

```bash
/usr/bin/python3 tools/ui_story.py list
/usr/bin/python3 tools/ui_story.py inspect tool-card/error
```

Capture is headless by default. The wrapper starts an isolated GTK Broadway display in the background, renders the requested component to a PNG, and terminates both processes. It does not open or focus a desktop window and does not use Spectacle:

```bash
/usr/bin/python3 tools/ui_story.py capture composer/running \
  --output artifacts/screenshots/composer-running.png
```

Pass `--visible` only when the compositor-rendered window itself is under review. Visible capture uses the existing AT-SPI readiness check and Spectacle workflow:

```bash
/usr/bin/python3 tools/ui_story.py capture composer/running \
  --visible --output artifacts/screenshots/composer-running-visible.png
```

Prefer a component story while changing layout, styling, empty states, loading states, errors, or long-content behavior. Use the production application only for bridge integration and end-to-end flows.

## Python helper functions

Reusable helpers live in `tools/ui_automation.py`. Run Python from the repository root:

```python
from tools.ui_automation import (
    application,
    click,
    descendants,
    find_node,
    replace_text,
    screenshot,
    visible_names,
    wait_for,
)
```

The helpers accept an `app_name` argument when automation targets an application other than `omp-native`. Generated screenshots are verification artifacts; add only intentional reference images to Git.


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
search = find_node(root=dialog, role="entry", name="Search models")
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
composer = find_node(root=app, role="text", name="Prompt")
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
