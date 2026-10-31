---
name: verify-change
description: Use after editing Zito's Rust code to build, test and lint it before reporting the work as done.
---

# Verify a Zito change

Run from the repo root, in this order, and report real results:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Notes
- Package-manager commands must not run during tests. Test pure parsers with fixture strings; `exec::capture` tests only use `sh` and a nonexistent binary.
- If `cargo` cannot fetch crates (no registry access), say so plainly. Do not claim the build passed; state which parts were checked another way.
- Every parsing, matching or orchestration bug fixed gets a regression test.
- For TUI changes also launch `cargo run --release` in a real terminal and check: search (`/`), `Tab` source cycling, a confirm dialog (`i`), and that `q` restores the terminal.
