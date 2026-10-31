---
name: spec-guard
description: Use before any change that alters Zito's product behaviour, TUI layout, keybindings, grouping rules, update or privilege handling, or anything the spec lists as an open requirement.
---

# Zito spec guard

The project documents are the source of truth: `What it is` (foundation spec) and `additionals to do now` (engineering direction). Both live in the Claude Project and are not in the repo.

## Before changing behaviour
1. Find the relevant section. Section 21 of `What it is` lists requirements that are deliberately **open**; do not silently turn an assumption into a requirement.
2. If the change touches an open item, either ask the user (state the ambiguity, the options and their consequences) or take the smallest reversible choice and record it in the README under "Decisions taken that the spec lists as OPEN".
3. Check section 19 (non-goals) and the "Avoid for now" list: no CLI before the TUI, no background daemons, no aggressive fuzzy matching, no automatic source migration, no rollback promises.

## Always true
- Keep the existing layout (module structure, panes, keys) unless the user asks for a change.
- Source transparency: every package keeps its manager identity. Wrongly merging packages is worse than showing them separately.
- Destructive or broad operations need explicit confirmation; detection (e.g. orphans) never removes anything.
- Failures are isolated per manager and reported honestly; never report full success after a partial failure.
- The project name is Zito (not MPM). Config lives in `~/.config/zito`, data in `~/.local/share/zito`.
