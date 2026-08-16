# Repository instructions

- `omp` is always lowercase. Oh My Pi is not part of this project; this repository only builds a frontend for it. Treat every local `omp` clone as read-only reference material: never modify, build, test, format, or run maintenance commands in it.
- Linux with GTK is the primary runtime and UI target. Assume features should remain cross-platform where practical: minimize platform-specific branches, keep unavoidable platform code behind narrow boundaries, and prefer the Rust standard library or cross-platform crates such as `rodio` for portable behavior.
- Before testing the native app UI, read and follow @docs/ui-automation.md.
- Response style: follow the vendored Attention-kind instructions in `.omp/APPEND_SYSTEM.md`. `omp` loads this automatically when launched from the repository root; other agents should read it before responding.
- Keep commits atomic and use Conventional Commit messages.

## Jujutsu change workflow

- Before working on any change, read and follow the installed `jj-stacks` skill at `skill://jj-stacks`. It is the source of truth for workspace creation, ownership, rebases, conflicts, handoffs, and recovery; these repository rules choose the default path through that skill.
- A change owner MUST create a uniquely named `jj workspace` before inspecting implementation files, editing, generating code, building, or testing. Create it outside the primary checkout, pin it to the primary workspace's current commit ID, and do all change-related work inside it. Never develop directly in the primary working tree and never share a workspace between agents.
- If `jj root` reports that this checkout is Git-only, this repository policy authorizes the one-time `jj git init` bootstrap from the primary checkout. Do not mix Git worktrees or mutating Git commands with the resulting shared jj repository.
- Keep an ordinary task as one atomic, described change. Run `jj status`, inspect the diff, resolve all conflicts, and complete focused verification in the task workspace before integration.
- When an ordinary task is done, it MUST be integrated back into the primary working tree. Hand off the verified tip change ID, stop mutating the task workspace, then adopt that tip from the primary workspace as described by `jj-stacks` (normally `jj new <tip-change-id>` when the task is based on the primary change). Confirm the primary working tree contains the result before forgetting and deleting the temporary workspace. A completed change left only in a temporary workspace is unfinished.
- Exception: when the requested deliverable is a change stack or a pull request, preserve its commit boundaries and keep its dedicated workspace instead of integrating it into the primary working tree. Follow the skill's handoff format and report the workspace path, base, root and tip change IDs, conflicts, verification, next owner, and any task bookmarks or pushed revisions.
