---
name: jj-stacks
description: Create and manage Jujutsu change stacks for single-threaded work and multi-agent orchestration, including jj workspaces, ownership handoffs, rebases, merges, conflict resolution, recovery, and stacked GitHub changes.
---

# Jujutsu stacks

Use this skill when work should be represented as one or more mutable jj changes, especially when several agents need isolated working copies or must integrate dependent changes.

Commands and behavior below target jj 0.44.

## Core model

- A **change ID** identifies a logical change across rewrites. Use change IDs in handoffs and orchestration; commit IDs change after rebases and other rewrites.
- A **stack** is an ordered chain of changes owned as a unit.
- A **workspace** is jj's native separate working copy. Use `jj workspace`, never `git worktree`, for a shared jj repository.
- Every mutable change or stack MUST have exactly one active owner.
- The owner of a stack performs its rebases, merges, conflict resolution, cleanup, and focused verification.
- A dispatcher/orchestrator assigns ownership, communicates change IDs and dependencies, and checks results. It MUST NOT become the default merge bottleneck for stacks it does not understand.
- When cross-stack integration is needed, designate an **integration owner** with relevant implementation context. That agent creates and validates the integration change.

Filesystem isolation does not prevent semantic conflicts. Reduce conflicts through ownership and task boundaries; use jj to preserve and expose the conflicts that remain.

## Non-negotiable safety rules

1. NEVER combine Git worktrees with a shared jj repository. Git worktrees are unsupported by jj.
2. NEVER let two live workspaces edit or rewrite the same change.
3. NEVER rewrite another live workspace's working-copy commit. That workspace will become stale.
4. NEVER run `jj rebase`, `jj squash`, `jj absorb`, `jj abandon`, or `jj edit` across a stack owned by another active agent.
5. NEVER use mutating Git commands in a jj workspace. Use `jj status`, `jj diff`, `jj log`, `jj git fetch`, and `jj git push`.
6. NEVER move a shared bookmark such as `main` from a worker. Workers may own uniquely named task bookmarks.
7. NEVER run project-wide formatting, code generation, or lockfile updates concurrently when they touch shared files. The stack or integration owner performs them after integration.
8. NEVER use `jj undo` blindly in a shared multi-agent repository. The newest operation may belong to another agent.
9. Do not choose a side of an ambiguous conflict without implementation context. Route it to a contributing stack owner.

## Repository detection and bootstrap

Check whether the current directory is already a jj workspace:

```bash
jj root
jj workspace list
```

If `jj root` says the directory is only a Git repository, do not initialize jj as an incidental worker action. Repository adoption is a bootstrap decision. When the task explicitly authorizes it, initialize from the primary working copy:

```bash
jj git init
jj git colocation status
```

The default colocated layout keeps `.git` in the primary workspace for Git-dependent tools. Secondary jj workspaces usually contain `.jj` but not `.git`; ordinary Git working-tree commands will not work there.

Confirm jj identity before creating publishable changes:

```bash
jj config get user.name
jj config get user.email
```

If missing, obtain the correct identity rather than inventing one, then configure it with `jj config set --user` or the appropriate narrower scope.

If the current working-copy change was created before identity was configured, create a fresh change or run `jj metaedit --update-author` before publishing it.

## Pin the batch base

Fetch once at a coordinated batch boundary, not independently from every worker:

```bash
jj git fetch
jj log -r 'trunk()'
```

Resolve the intended base to a commit ID and pass that immutable value to every independent worker:

```bash
BASE=$(jj log -r 'trunk()' --no-graph -T 'commit_id ++ "\n"')
```

If `trunk()` is not configured, use the explicit remote bookmark or revision selected for the task. Do not repeatedly resolve a moving bookmark while creating sibling workspaces.

## Create an owned workspace

Put secondary workspaces outside the primary repository working tree. Give every workspace a unique name and path:

```bash
jj workspace add ../repo-jj-workspaces/agent-auth \
  --name agent-auth \
  -r "$BASE" \
  -m "agent-auth: implement authentication"
```

The new workspace starts with its own empty working-copy change on the requested base. The agent owns that change immediately.

For agent tasks, the workspace contract should include:

- Repository and workspace paths.
- Unique workspace name.
- Pinned base commit ID.
- Scope and files or symbols owned.
- Required interfaces and acceptance criteria.
- Dependency change IDs, if any.
- Whether the result should remain parallel, become a linear stack, or produce a merge change.
- The agent responsible for cross-stack integration.

Do not put multiple agents in the same workspace.

## Single-threaded stack workflow

### Start and describe work

If `jj workspace add` already created the current change, edit it directly. Otherwise start from the selected base:

```bash
jj new <base> -m "feat: first logical change"
```

Review current work at any time:

```bash
jj status
jj diff
```

Descriptions should identify coherent reviewable changes, not temporary activity.

### Add a stack layer

Finish the current change and create a new empty child:

```bash
jj commit -m "refactor: isolate storage boundary"
```

Continue editing the new `@`. For the final layer, prefer `jj describe` instead of creating an unnecessary empty child:

```bash
jj describe -m "feat: add storage-backed behavior"
```

Inspect the owned stack:

```bash
jj log -r '<base>..@'
```

Record the root and tip change IDs before handoff.

### Fix an earlier layer

Prefer a visible fixup change, review it, then move it into the intended parent:

```bash
jj new <target-change> -m "fixup: correct boundary behavior"
# edit and verify
jj diff
jj squash
```

`jj absorb` is useful when a fixup contains hunks belonging to several mutable ancestors:

```bash
jj absorb -f <fixup-change> -t '<owned-stack-revset>'
```

Ambiguous hunks remain in the source change. Review the resulting operation with:

```bash
jj op show -p
```

Use `jj split -r <change>` when a change contains multiple logical units. Use filesets for obvious path separation or interactive mode for mixed hunks.

### Rebase an owned stack

Move the stack from its root so all descendants retain their internal order:

```bash
jj rebase -s <stack-root> -o <new-base>
```

Only the stack owner runs this while its workspace is live. Afterward:

```bash
jj log -r 'conflicts() & ::@'
jj status
```

The owner resolves and verifies any conflicts caused by the new base.

## Multi-agent orchestration

### Independent work: create sibling stacks

Independent agents start from the same pinned base:

```text
       stack A
      /
base ── stack B
      \
       stack C
```

Do not prematurely serialize them. A false dependency makes later rewrites cascade through unrelated work.

The dispatcher may inspect logs and summaries, but each stack owner retains graph ownership:

```bash
jj log -r '<base>..'
jj diff --summary -r <change>
```

### Real dependency: consumer owns the rebase

If stack B consumes stack A, choose one of two flows.

Preferred when the dependency is known before B starts:

```bash
jj workspace add ../repo-jj-workspaces/agent-b \
  --name agent-b \
  -r <A-tip> \
  -m "agent-b: implement dependent behavior"
```

If B started in parallel from the old base, wait until A is stable. Then B's owner rebases B's own stack:

```bash
jj rebase -s <B-root> -o <A-tip>
```

B's owner resolves the conflicts because B's implementation is the code being adapted to A. A remains unchanged and its workspace does not become stale.

### Independent results that need a merge

Choose one contributing agent as integration owner. In that agent's workspace:

```bash
jj new <A-tip> <B-tip> -m "integrate: combine A and B"
```

jj records unresolved file conflicts in the merge change instead of stopping midway. The integration owner resolves them in the working copy, reviews the integration diff, and runs the combined verification:

```bash
jj status
jj diff
# edit conflict markers or use jj resolve
jj status
```

If several contributions overlap heavily, integrate them in context-aware stages instead of creating a large many-parent conflict. Give each stage an owner.

### Linear history required after parallel work

The owner of the downstream stack performs the linearization:

```bash
jj rebase -s <B-root> -o <A-tip>
```

For `A -> B -> C`, B owns the first rebase and C owns the second. If one agent is explicitly responsible for the final combined feature, transfer integration ownership to that agent and let it perform both operations only after the source stacks are quiescent.

Do not make the dispatcher execute a sequence of rebases merely because it can see all change IDs. Visibility is not implementation context.

When the harness supports peer messaging, the producing owner SHOULD send its change IDs and handoff directly to the downstream or integration owner. The dispatcher records the dependency and outcome; it does not replay patches or perform a context-free merge. Prefer a contributing agent or downstream consumer over a newly spawned integration-only agent that starts without implementation context.

### Separate changes that were accidentally stacked

If revisions are actually independent, their owner can make them siblings:

```bash
jj parallelize '<first>::<last>'
```

Review descendants after this graph rewrite.

## Ownership transfer

A mutable stack changes owners only through an explicit handoff:

1. Current owner snapshots with a jj command, reviews `jj status` and `jj diff`, and describes every change.
2. Current owner reports root and tip change IDs, verification, and unresolved risks.
3. Current owner stops mutating the workspace.
4. Receiver acknowledges the handoff before rewriting anything.
5. If the receiver must edit the exact working-copy change, forget the old workspace first so it cannot become stale.

From another workspace:

```bash
jj workspace forget <old-workspace-name>
```

The change remains visible as an anonymous head. The receiver can create a new workspace on top of it:

```bash
jj workspace add <new-path> --name <new-name> -r <tip-change>
```

Prefer adding a child fix and squashing it after review. Use `jj edit <change>` only when direct ownership of that exact change has been transferred and no other live workspace owns it.

`jj workspace forget` does not delete files. Delete the old directory separately only after confirming the handoff is durable.

## Conflict ownership and resolution

The agent whose stack is being adapted owns rebase conflicts. The designated integration owner owns merge conflicts.

Find conflicts in the relevant ancestry:

```bash
jj log -r 'conflicts() & ::@'
```

For a conflicted historical change, resolve in a child so the resolution is reviewable:

```bash
jj new <conflicted-change> -m "resolve: integrate conflicting behavior"
# resolve markers or run jj resolve
jj diff
jj squash
```

For a newly created merge working-copy change, resolve directly in that owned merge change and inspect `jj diff` before completion.

Completion requires:

- No unresolved conflicts in the owned result.
- Combined behavior, not merely conflict-marker removal.
- Focused tests for the integrated path.
- Formatter/code generation run once after semantic integration, when applicable.

## Handoff format

Every worker or integration owner returns a concrete handoff:

```text
workspace: <name and path>
base: <base commit or change ID>
stack-root: <change ID>
stack-tip: <change ID>
changes:
  - <change ID> <description>
integrated-inputs:
  - <change ID or none>
conflicts: <none, resolved details, or unresolved paths>
verification:
  - <exact command or scenario and result>
next-owner: <agent, user, or none>
```

Also provide these machine-readable facts when useful:

```bash
jj log -r @ --no-graph \
  -T 'change_id ++ " " ++ commit_id ++ " " ++ description.first_line() ++ "\n"'
jj diff --summary -r @
```

A task is not handed off merely because files exist in a workspace. The changes must be described, identified, and verified.

## Bookmarks and stacked reviews

Keep local agent work anonymous until publication unless a stable external name is needed. For GitHub stacked reviews, the stack owner creates one unique bookmark per published layer:

```bash
jj bookmark create agents/feature-a/base -r <A>
jj bookmark create agents/feature-a/behavior -r <B>
jj git push --bookmark agents/feature-a/base
jj git push --bookmark agents/feature-a/behavior
```

Set the first PR's base to the trunk branch and each later PR's base to the prior bookmark.

Bookmarks follow a change when that change is rewritten. They do not advance automatically when a new child is created. The stack owner is responsible for moving its unique bookmarks. Workers MUST NOT move shared release or trunk bookmarks.

## Recovery in a shared repository

Inspect operations without snapshotting a working copy or reconciling new operations:

```bash
jj --at-op=@ --ignore-working-copy op log
```

Use `jj evolog -r <change>` to understand how a logical change evolved.

When reverting an operation, identify the exact operation created by the current owner:

```bash
jj op revert <operation-id>
```

Do not use `jj undo` unless you have verified that the latest operation is yours and no concurrent operation will be reverted.

If a workspace is stale:

```bash
jj workspace update-stale
```

Stop first and identify the foreign rewrite. Updating stale state is recovery, not a normal synchronization mechanism.

If a change is divergent, do not select `/0` or `/1` arbitrarily. Determine whether to abandon one side, assign a new change ID to one side, or combine them. Divergence usually means the single-owner invariant was violated; fix the ownership process as well as the graph.

## Choosing workspaces versus full clones

Use shared jj workspaces when agents can use jj commands and benefit from immediate visibility of each other's changes.

Use independent full `jj git clone` repositories when:

- Every agent requires a real `.git` working tree.
- Agents must independently fetch or mutate Git state.
- Hard repository isolation is required.
- The repository is shared through NFS, Dropbox, or another distributed filesystem.

Full clones isolate operation logs and stale-workspace behavior but require bookmark-based handoffs or fetches. They do not eliminate content conflicts.

## Completion checklist

Before yielding an owned stack or integration result:

- [ ] All changes have accurate descriptions.
- [ ] Root and tip change IDs are recorded.
- [ ] Only owned changes were rewritten.
- [ ] `jj status` reports no unresolved conflict in the delivered result.
- [ ] The final graph matches the intended dependency structure.
- [ ] Focused behavior was exercised after the last rebase or merge.
- [ ] Shared formatting, generated files, and lockfiles were updated once by the responsible owner.
- [ ] Bookmark changes, if any, are unique to the owned stack.
- [ ] The next owner and remaining risks are explicit.
