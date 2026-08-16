# Native UI feature parity priorities

This document tracks user-facing omp features that are missing or incomplete in the native frontend. The native app communicates with omp through `omp --mode rpc-ui`; the interactive omp UI remains the parity reference.

Priority reflects user impact, not implementation difficulty. The six highest-priority areas below should be completed before the remaining parity backlog.

## Highest priority

### 1. Image prompting

**Outcome:** users can attach images to a prompt and see exactly what will be sent before submission.

Current state:

- omp RPC already accepts `images?: ImageContent[]` on `prompt`, `steer`, `follow_up`, and `abort_and_prompt`.
- `ImageContent` contains base64 data and a MIME type.
- The native bridge sends only `{ message }`; the composer has no attachment state or image controls.
- Native can render images returned by tools, but that does not provide image input.

Required work:

- Add composer attachment state independent of the text buffer.
- Support an attachment button and file chooser.
- Support pasting image data from the clipboard. Drag-and-drop may use the same ingestion path.
- Show each pending image as a removable preview with an accessible name and no opaque strip behind unused preview space.
- Encode accepted images as RPC `ImageContent` payloads without blocking GTK's main thread.
- Preserve the draft and attachments when encoding or submission fails.
- Clear attachments only after the bridge accepts the request.
- Reject unsupported or unreadable files with a visible, actionable error.

Acceptance criteria:

- A PNG or JPEG selected from disk appears in the composer and reaches omp in the next prompt.
- A pasted image follows the same path.
- Multiple attachments preserve their displayed order.
- Removing one attachment does not alter the others or the prompt text.
- Submission failure leaves the complete draft intact.
- AT-SPI exposes the attachment button, attachment names, and remove actions.
- A component story covers empty, populated, multiple-image, and error states; a production bridge check proves the RPC payload reaches omp.

Protocol dependency: none for basic support. The required image fields already exist in `modes/rpc/rpc-types.ts`.

### 2. Composer improvements

**Outcome:** the composer remains the primary control surface before and during a turn instead of becoming only a stop button while omp is working.

Current state:

- Native supports multiline text, send/stop, model selection, thinking level, and static slash/subcommand completion.
- While an agent is running, the primary action aborts the turn. Native cannot steer, queue a follow-up, inspect queued input, or remove queued input.
- omp RPC already exposes `steer`, `follow_up`, `abort_and_prompt`, queue-mode setters, and queue state.
- Native deserialization currently drops `steeringMode`, `followUpMode`, `interruptMode`, and `queuedMessageCount` from `get_state`.
- omp's interactive composer also supports file/internal-URL/GitHub/emoji completion, extension autocomplete, prompt history search, external editing, and large-paste attachment.

Required work, in order:

1. Separate **Send/Queue** from **Stop** while a turn is running.
2. Send active-turn input through `steer` or `follow_up` according to an explicit composer mode.
3. Display the queued-message count and expose queued-message removal when the protocol can identify queued entries.
4. Deserialize and present steering, follow-up, and interrupt modes from `get_state`.
5. Add image attachment controls from priority 1.
6. Extend completion beyond the current static slash metadata:
   - argument and path completion;
   - internal URLs;
   - GitHub issue and pull-request references;
   - extension-provided completions;
   - optional emoji completion.
7. Add prompt-history search and robust draft retention across dialogs, session switches, and failed requests.
8. Add large-paste handling and external-editor integration after the core running-turn flow is stable.

Acceptance criteria:

- Text can be entered and submitted without aborting an active turn.
- The selected running-turn behavior is explicit and survives state refreshes.
- Stop remains independently accessible.
- Queue count updates from authoritative omp state rather than optimistic local bookkeeping.
- Draft text survives failed requests and interactive dialogs.
- Completion never inserts a command unavailable in `rpc-ui`.
- Stories cover ready, running with an empty draft, running with a draft, queued, disconnected, and attachment states.
- Production automation proves one steering message and one follow-up message reach omp without ending the active turn.

Protocol dependency: existing RPC covers steering and follow-up. Dequeue-by-ID and richer dynamic completion metadata may require new protocol operations.

### 3. Todos

**Outcome:** todo progress stays visible without consuming conversation space or making the user responsible for managing agent work.

Current state:

- omp includes authoritative `todoPhases` in `get_state`.
- `/todo` remains available to the agent through the RPC text handler.
- Native renders a progress rail over the conversation and reveals plan detail on hover, focus, or activation.

Required work:

- Deserialize todo phases and every task state from `get_state`.
- Keep the surface informational: no initialize, append, start, complete, drop, block, unblock, remove, or clear controls.
- Use omp's bounded walking-window policy: hide completed phases, retain one recently closed task for context, show current open work, and summarize later phases.
- Keep task states visually and semantically distinct without treating the UI as the source of progress.
- Replace displayed data only from authoritative omp state. Do not infer task completion from tool cards or assistant prose.

Acceptance criteria:

- Existing todos appear after startup and session switching.
- A compact rail communicates overall completion without changing conversation or composer layout.
- Hover, keyboard focus, and activation reveal the bounded plan detail.
- Completed work disappears except for one recent context row; open work remains ordered and visible.
- In-progress, blocked, completed, and abandoned tasks are visually and semantically distinct.
- Empty plans do not reserve space.
- Component stories cover empty, multi-phase, blocked, completed, active, and long-task states.
- A production bridge check confirms that `get_state.todoPhases` renders without exposing todo mutation actions.

Protocol dependency: none for read-only plan display.

### 4. Vibe, goal, and loop modes

**Outcome:** session modes are first-class native state with clear activation, constraints, progress, and exit controls.

Current state:

- `/vibe`, `/goal`, `/guided-goal`, and `/loop` are TUI-only commands. They are neither advertised nor dispatched in `rpc-ui`.
- Their state and orchestration currently live in `InteractiveMode`, not the transport-neutral RPC session state.
- Manually submitting these commands in native can forward the text to the model as an ordinary prompt instead of activating a mode.

Required shared foundation:

- Move mode orchestration behind transport-neutral session APIs in omp.
- Add RPC mode-state snapshots and change events.
- Add explicit RPC operations instead of teaching native to emulate TUI command behavior.
- Surface the active mode as a persistent composer/header chip with status and a direct exit action.
- Return structured errors for mutually exclusive or disabled modes.

#### Vibe mode

Required behavior:

- Enable or disable vibe mode, optionally with an initial prompt.
- Report active state and any model/worker configuration relevant to the user.
- Preserve the mode across normal state refreshes and show why activation is blocked by another mode.

#### Goal mode

Required behavior:

- Set or replace an objective.
- Show objective, status, progress, and token budget.
- Pause, resume, drop, and adjust the budget.
- Support guided-goal interviewing through normal chat and question surfaces.
- Make goal completion and failure visible without relying only on notification sounds.

#### Loop mode

Required behavior:

- Enable or disable loop mode with an optional count or duration limit.
- Accept an inline prompt or wait for the next composer submission.
- Show waiting, active, paused, iteration, and limit state.
- Allow the current iteration to be interrupted without silently discarding the configured loop.

Acceptance criteria:

- Mode state is restored after native reconnects to the same omp process.
- Impossible transitions return structured failures and do not alter local UI optimistically.
- The active chip always matches omp's authoritative state.
- Goal pause/resume/drop and loop enable/disable work without slash commands.
- Guided goal can invoke the native question UI.
- Stories cover inactive, active, blocked, paused, completed, and error states for each mode.
- Production bridge checks exercise one complete state transition for vibe, goal, and loop.

Protocol dependency: required. These modes are not currently exposed by `rpc-ui`.

### 5. Agent hub

**Outcome:** native provides both an operational runtime hub for live agents and a configuration hub for available agent definitions.

Current state:

- Native subscribes to subagent lifecycle/progress/events and shows subagent chips.
- Users can open a subagent transcript.
- Native does not provide omp's full runtime roster/tree, unread state, metrics, direct chat/steer, revive, abort/release, or persistent-agent history.
- Native does not provide the `/agents` configuration hub for enabling agents or selecting per-agent model, prewalk, and advisor behavior.

#### Runtime hub

Required work:

- Show every registered agent except Main in roster and parent/child tree views.
- Include status, current or last task, last activity, unread count, model/role, context/cost metrics when available, and child relationships.
- Open a live transcript from a selected row.
- Support direct chat or steering to an agent.
- Support revive for parked agents and abort/release for active or retained agents with confirmation where destructive.
- Include persisted historical subagents after restart.

#### Agent configuration hub

Required work:

- List agents grouped by source: project, user, and bundled.
- Enable or disable configurable agents.
- Edit per-agent model, prewalk, and advisor overrides with pickers rather than free-form command syntax.
- Support agent creation only after omp exposes a transport-neutral operation and validation contract.

Acceptance criteria:

- The runtime roster reconciles lifecycle events without duplicate agents.
- Parent/child relationships and active counts match omp's registry.
- Opening a transcript does not stop its live updates.
- A message sent from the hub reaches the selected agent, including revival of a parked agent when supported.
- Destructive actions clearly identify the target agent.
- Configuration values round-trip through omp and refresh discovery where required.
- Stories cover empty, running tree, unread, parked, failed, long-task, and transcript states.
- Production bridge checks cover transcript loading plus one supported lifecycle or messaging action.

Protocol dependency: partial. Existing subagent snapshots/events/messages cover the current read-only subset. Roster metadata, unread state, chat, revive, release, metrics, persisted discovery, and configuration operations require additional RPC support.

### 6. Session tree

**Outcome:** users can inspect and navigate branches inside the current session, then branch or fork from a selected message without confusing those operations with switching session files.

Current state:

- Native's sidebar and history dialog switch between session files.
- Native has no intra-session branch tree.
- omp RPC already exposes `get_branch_messages` and `branch`, which can support a basic branch-from-message picker.
- omp also exposes `handoff`; native does not consume it.
- The full TUI tree includes active-path highlighting, branch connectors, filters, search, labels, and optional summarization, but no equivalent full-tree RPC snapshot exists.

Required work, in milestones:

1. Add a branch-from-message picker backed by `get_branch_messages` and `branch`.
2. Add explicit fork and handoff actions with clear descriptions of their different semantics.
3. Add a full session-tree RPC snapshot with stable entry IDs, parent IDs, entry kind, display text, labels, active leaf, and selectable/summarizable capabilities.
4. Build a native tree view with active-path highlighting, search/filtering, branch connectors, and message previews.
5. Support switching to a selected branch and editing a branch label when omp exposes those operations.

Acceptance criteria:

- Session-file switching and intra-session branch navigation remain visibly separate concepts.
- Branching from a message uses its stable entry ID, not display text or child index.
- The active leaf and its ancestor path are distinguishable.
- Switching branches refreshes messages, title/state, todos, modes, and subagents as one session transition.
- Canceling a branch/fork/handoff operation leaves the current session unchanged.
- Stories cover a linear session, one fork, multiple roots, long labels, filtered results, and the active path.
- A production bridge check branches from a known message and confirms the returned session state.

Protocol dependency: existing RPC supports the first milestone. Full tree parity requires new snapshot and mutation operations.

## Remaining parity backlog

These remain important, but follow the six priorities above.

| Area | Native status | Required parity | Primary dependency |
|---|---|---|---|
| Rich question tool | Functional but generic | Structured multi-question dialog, descriptions, previews, notes, review screen, countdown, correct timeout dismissal | Extend extension UI RPC with `askDialog` |
| omp settings | Missing; native Settings only covers alerts and sound packs | Schema-driven Appearance, Model, Interaction, Context, Memory, Files, Shell, Tools, Tasks, and Providers pages | Settings schema/value/update RPC |
| Plan mode and plan review | Missing | Enter/exit plan mode; review, edit, refine, approve, compaction choice, execution transition | Transport-neutral plan-mode API and events |
| Provider setup and OAuth | Missing | Provider status, setup, login, logout, account selection/pinning, manual callback input | Existing login RPC plus logout/pinning/setup additions |
| Extension control center | Missing | Extension inventory, status, enable/configuration, errors, reload | Structured extension RPC |
| Plugin marketplace | Text-command fallback only | Browse, search, install, uninstall, enable, disable, update, progress/errors | Structured marketplace/plugin RPC |
| MCP management | Text-command fallback only | Server list/status, add wizard, test, enable/disable, OAuth, resources/prompts | Structured MCP RPC |
| SSH management | Text-command fallback only | Host list, add/remove, scope, validation, connection state | Structured SSH RPC |
| Usage and account reset | Text-command fallback only | Provider usage, limits, reset selector, account targeting | Structured usage/account RPC |
| Advisor configuration | Text-command fallback only | Enable/status, role/model configuration, transcript/dump affordances | Settings plus structured advisor state |
| Collaboration | Missing | Host/view links, join/leave, participant state, remote agent/question interaction | Collaboration RPC and event model |
| Speech-to-text and live voice | Missing | Recording state, transcription, submit trigger, realtime voice lifecycle | Native audio integration and RPC mode support |
| Pause/resume all agents | Missing | Freeze and resume main, subagents, and advisor with explicit state | Transport-neutral pause operation |
| Debug tools | Missing | Debug selector, log viewer, report actions | Structured debug RPC or native-safe host actions |
| Memory maintenance | Text-command fallback only | Status and maintenance controls | Structured memory RPC |
| Copy selector | Partial; per-message copy/quote exists | Conversation-wide text/code target picker | Message metadata already available; mostly native work |
| Export/share/dump | Text output or external launch | Native save/share progress, resulting path/link, failures | Existing commands plus structured result handling |
| Model service controls | Partial | Fast mode and computer/vision state controls | Existing `set_fast_mode`; add missing structured state where needed |
| Compaction and retry controls | Missing as native controls | Manual/automatic compaction, retry state, abort retry | Existing RPC operations and additional events as needed |
| Notifications from omp | Partial | Align ask/error/resource notifications with omp settings without conflating them with native sound preferences | Settings RPC and event classification |
| Extension UI parity | Partial | Working messages, richer widgets, custom surfaces where toolkit-neutral | Extend RPC; TUI component factories cannot cross the boundary directly |

## Slash-command compatibility

The audited omp source contains 70 top-level built-ins. Thirty-four have an RPC/text handler; 36 are TUI-only and are intentionally omitted from `get_available_commands`:

```text
settings setup plan plan-review vibe goal guided-goal loop queue switch
hotkeys extensions agents branch fork tree login logout
new clear drop handoff resume btw tan omfg retry debug exit
collab join leave copy
live pause quit
```

Native equivalents already cover parts of `/switch`, `/new`, `/resume`, `/drop`, `/copy`, `/exit`, and `/quit`; do not duplicate those solely to match command names.

For the rest, native must not advertise unsupported commands. If a user manually enters a known TUI-only command, the frontend or omp RPC dispatcher should return a structured unsupported-command error instead of forwarding it to the model as prompt text.

Commands with both handlers can still lose their TUI-specific surface in RPC mode. Notable examples are `/todo`, `/session`, `/usage`, `/mcp`, `/ssh`, `/advisor configure`, and `/marketplace`. Prefer structured native controls over reproducing their command-line grammar.

## Existing native coverage to preserve

Parity work should extend rather than replace these working native surfaces:

- Conversation and session-file sidebar/history
- New, switch, rename, reveal, and delete conversation flows
- Streaming assistant text and thinking
- Markdown and tool cards, including GFM tables, native LaTeX, Mermaid diagrams, and images returned by tools
- Model picker and thinking-level picker
- Static slash/subcommand completion for commands advertised by omp
- Stop/abort
- Context usage, token rate, cost, and workspace display
- Subagent chips and read-only transcript view
- Per-message copy and quote actions
- Generic extension `select`, `confirm`, `input`, and `editor` dialogs
- Extension notices, status text, string widgets, editor text updates, and URL launching
- Native desktop notifications, sounds, and sound-pack settings

## Protocol and implementation rules

- omp remains authoritative for session, mode, queue, todo, agent, and branch state.
- Do not edit omp configuration or session JSONL files directly from native UI.
- Prefer explicit RPC operations and structured events over submitting slash-command strings.
- Do not expose a control until its current state and failure result can be reconciled from omp.
- Keep native-only preferences, such as sound packs, separate from omp settings.
- Reject known unsupported commands rather than letting them become model prompts.
- New dialogs, panels, and stateful controls require component stories.
- Use AT-SPI for semantic interaction checks and a production `rpc-ui` smoke check for every bridge-dependent flow; see [UI automation and screenshots](ui-automation.md).

## Completion definition

A parity item is complete only when:

1. The native surface implements the full user-visible transition, including cancel and error paths.
2. State survives refresh, reconnect, and session switching where the omp feature does.
3. The protocol carries structured state and results; no UI behavior depends on parsing rendered command output.
4. Accessibility names and actions support coordinate-free AT-SPI automation.
5. Component stories cover meaningful visual states.
6. A production bridge check proves the changed RPC path against omp.
