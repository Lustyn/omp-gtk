# Repository instructions

- `omp` is always lowercase. Oh My Pi is not part of this project; this repository only builds a frontend for it. Treat every local `omp` clone as read-only reference material: never modify, build, test, format, or run maintenance commands in it.
- Linux with GTK is the primary runtime and UI target. Assume features should remain cross-platform where practical: minimize platform-specific branches, keep unavoidable platform code behind narrow boundaries, and prefer the Rust standard library or cross-platform crates such as `rodio` for portable behavior.
- Before testing the native app UI, read and follow @docs/ui-automation.md.
- Before responding, read and follow @docs/attention-kind.md.
- Keep commits atomic and use Conventional Commit messages.

## Git worktree workflow

- The primary checkout stays on `main`. Agents MUST create a uniquely named branch and linked worktree under the repository's ignored `.worktrees/` directory before inspecting implementation files, editing, generating code, building, or testing.
- Create the task worktree from the current local `main` with `git worktree add .worktrees/<task-name> -b <type>/<task-name> main`, then perform the entire task inside that worktree. One owner uses one worktree and branch.
- Keep each task as one atomic, described change with a real author and a Conventional Commit message. Before integration, inspect `git status` and `git diff`, resolve every conflict, and complete focused verification in the task worktree.
- Integration is serialized. Rebase the task branch onto the latest local `main` inside its worktree and repeat focused verification there. From the clean primary checkout on `main`, integrate with `git merge --ff-only <task-branch>`.
- After the fast-forward succeeds, remove the linked checkout with `git worktree remove .worktrees/<task-name>` and delete the merged task branch with `git branch -d <task-branch>`.
- If the primary checkout is not clean and attached to `main`, leave it untouched and block integration until its owner resolves the state.
- For a requested pull request or multi-commit stack, preserve the task branch and worktree until the review lifecycle is complete instead of integrating and deleting them locally.
