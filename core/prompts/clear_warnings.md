# Clear Warnings and Clippy

Fix all compiler warnings and clippy lints in this project without changing behavior. Work in this order:

## 1. Build warnings

```bash
task build_dev
```
Fix all warnings (unused imports, dead code, unused variables, etc.). Remove or use the flagged items. If a warning is intentional because of platform-conditional code, preserve the code and explain it.

## 2. Clippy
```bash
cargo clippy --all-features -- -D warnings
```
Fix all clippy lints. Do not use `#[allow(clippy::...)]`.

## Rules
- Run each command, fix all issues, and re-run it to confirm zero warnings or errors.
- Do not run `cargo fmt` or core tests.
- Do not introduce behavioral changes; only clean up warnings and lints.
- Keep the diff minimal and review unrelated worktree changes before editing.
