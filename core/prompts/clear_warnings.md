# Clear Warnings, Clippy & Formatting

Fix all compiler warnings, clippy lints, and formatting issues in this project. Work in this order:

## 1. Formatting
```bash
cargo fmt --all -- --check
```
Fix any formatting issues reported. run `cargo fmt` directly. 

## 2. Build Warnings
```bash
task build_dev
```
Fix all warnings (unused imports, dead code, unused variables, etc). Remove or use the flagged items. If unsure how to handle them leave them as is and say it.

## 3. Clippy
```bash
cargo clippy --all-features
```
Fix all clippy lints. Do not use `#[allow(clippy::...)]`.

## Rules
- Run each command, fix all issues, re-run to confirm zero warnings/errors.
- Do NOT introduce behavioral changes — only clean up.
- Use subagents to parallelize independent fixes across files.
- If a warning is intentional (e.g. platform-conditional code), add a targeted `#[allow(...)]` with a comment explaining why.
