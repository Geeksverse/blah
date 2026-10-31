---
name: add-provider
description: Use when adding or changing a package-manager provider in Zito (a new file under src/managers/ implementing the Manager trait), such as Snap, pip or pipx.
---

# Adding a Zito provider

Zito delegates every operation to the real package manager's CLI. A provider only builds commands and parses output.

## Steps
1. Check the spec first (`What it is`, sections 2.3, 19 and 21): the provider list is a priority, not a promise. If the manager is not in the current MVP list, confirm with the user before adding it.
2. Add `ManagerKind::<Name>` in `src/model.rs` (`ALL`, `label()`, `groupable()`). Only desktop-application sources may be `groupable()`; language-ecosystem managers never merge with system packages.
3. Create `src/managers/<name>.rs` implementing `Manager`: `kind`, `binary`, `search`, `installed`, `updates` (or `supports_updates() -> false` with a visible reason), `info`, `plan`. Register it in `managers::build` and add a colour in `ui::label_color`.
4. Commands go through `CommandSpec::new(program, &[args])`. Never build shell strings. Use `.root()` only when the manager truly needs root; helpers and user-level managers handle their own elevation.
5. Use `exec::capture` for reads. Parse defensively: empty output, extra header lines, non-UTF-8, and missing fields must not panic or fabricate metadata.
6. Put any parsing into a pure `fn parse_*(&str) -> Vec<..>` so it can be unit-tested with fixture output. Cover empty, malformed, and representative output.
7. Add the manager to `config.example.toml` (`source_priority`) and the README keys/decisions if behaviour is a judgment call.

## Do not
- Reimplement downloads, dependency resolution or installation.
- Treat a launched process as success; judge by exit status and output.
- Silently change an existing installation's source.
