# Repository instructions

- `omp` is always lowercase. Oh My Pi is not part of this project; this repository only builds a frontend for it. Treat every local `omp` clone as read-only reference material: never modify, build, test, format, or run maintenance commands in it.
- Linux with GTK is the primary runtime and UI target. Assume features should remain cross-platform where practical: minimize platform-specific branches, keep unavoidable platform code behind narrow boundaries, and prefer the Rust standard library or cross-platform crates such as `rodio` for portable behavior.
- Before testing the native app UI, read and follow @docs/ui-automation.md.
- Keep commits atomic and use Conventional Commit messages.
