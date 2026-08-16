# Repository instructions

- `omp` is always lowercase. Oh My Pi is not part of this project; this repository only builds a frontend for it. Treat every local `omp` clone as read-only reference material: never modify, build, test, format, or run maintenance commands in it.
- Linux with GTK is the primary runtime and UI target. Assume features should remain cross-platform where practical: minimize platform-specific branches, keep unavoidable platform code behind narrow boundaries, and prefer the Rust standard library or cross-platform crates such as `rodio` for portable behavior.
- Before testing the native app UI, read and follow @docs/ui-automation.md.
- Response style: follow the vendored Attention-kind instructions in `.omp/APPEND_SYSTEM.md`. `omp` loads this automatically when launched from the repository root; other agents should read it before responding.
- Keep commits atomic and use Conventional Commit messages.

## Jujutsu change workflow

- Before working on any change, read and follow the installed `jj-stacks` skill at `skill://jj-stacks`. It is the source of truth for workspace creation, ownership, rebases, conflicts, handoffs, and recovery; these repository rules choose the default path through that skill.
- Use jj's native [`workspace`](https://docs.jj-vcs.dev/latest/cli-reference/#jj-workspace) command for every change; `jj help workspace` is the authoritative local reference. Create a uniquely named secondary workspace outside the primary checkout from an immutable commit ID resolved from `main`, and perform all inspection, editing, generation, building, testing, integration, and landing there. One owner uses one workspace.
- If `jj root` reports that this checkout is Git-only, this repository policy authorizes the one-time `jj git init` bootstrap from the primary checkout. Do not mix Git worktrees or mutating Git commands with the resulting shared jj repository.
- Keep an ordinary task as one atomic, described change with a real author. Run `jj status`, inspect the diff, resolve all conflicts, and complete focused verification in the task workspace.
- Landing on `main` is serialized. The task or integration owner resolves the latest `main`, rebases the owned stack onto that immutable commit, resolves conflicts, and repeats focused verification in the secondary workspace. The designated landing owner then advances `main` with `jj bookmark move main --to <verified-tip>` and runs `jj git export` so the colocated Git branch resolves to the same commit.
- After `main` resolves to the verified tip, forget the current task workspace with `jj workspace forget`, stop using that directory, and delete it from outside the workspace. The primary working tree remains untouched for the entire task.
- For a requested change stack or pull request, preserve its commit boundaries and dedicated workspace instead of landing it on `main`. Follow the skill's handoff format and report the workspace path, base, root and tip change IDs, conflicts, verification, next owner, and task bookmarks or pushed revisions.
