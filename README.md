# MPM — Meta Package Manager (TUI MVP)

Rust + Ratatui. Search once, discover everywhere, manage from one place.
Every operation is delegated to the real package manager's CLI.

## Build & run
```bash
cargo run --release
# optional config
mkdir -p ~/.config/mpm && cp config.example.toml ~/.config/mpm/config.toml
```
Needs Rust 1.74+. Optional: `pacman-contrib` (checkupdates), `yay` or `paru`.

## Keys
| Key | Action |
|---|---|
| `/` | search all ready managers (Enter submits) |
| `1`–`6` | Search · Installed · Updates · Orphans · History · Managers |
| `j k ↑ ↓` `g G` `^d ^u` | navigate |
| `Tab` / `S-Tab` | cycle the selected software's package source |
| `f` | cycle manager filter |
| `d` / `Enter` | load full package details |
| `i` `r` `U` | install / remove / upgrade selected source (always confirmed) |
| `A` | upgrade all managers (isolated; partial failure reported) |
| `R` | refresh view · `e` toggle manager (Managers view) · `?` help · `q` quit |

## Layout
```
src/model.rs        domain types (ManagerKind, PackageEntry, SoftwareIdentity, Op, OpStatus…)
src/managers/       Manager trait + pacman, aur (yay/paru), flatpak, npm, cargo
src/identity.rs     software-identity grouping (conservative)
src/app.rs          state, keys, worker threads, plan/confirm/execute
src/ui.rs           Ratatui rendering
src/exec.rs         process helpers, interactive runner (sudo/prompts work)
src/history.rs      JSONL history at ~/.local/share/mpm/history.jsonl
src/config.rs       ~/.config/mpm/config.toml (priorities, disabled, aur_helper)
```
CLI hook: `Manager::plan()` and `App`'s search/installed/update logic are UI-free,
so a future `mpm search|install|…` can reuse them.

## Decisions taken that the spec lists as OPEN (please review)
1. **Identity matching**: pacman/aur/flatpak merge only on exact normalized name; npm/cargo never merge.
2. **Config format**: TOML.
3. **Privilege**: only pacman ops get `sudo`; helpers/flatpak/npm/cargo use their own model. Mutating commands run with the real terminal attached so prompts work.
4. **Pacman single-package upgrade** runs `pacman -Syu` (Arch forbids partial upgrades); the confirm dialog says so.
5. **Update preview**: updates are listed in the Updates view; executing always goes through a confirm dialog.
6. **Global update order**: by source priority; failures never stop the rest.
7. **Cancellation**: Ctrl-C during an operation cancels the child, not MPM; recorded as `cancelled`.
8. **Cargo**: no update check (needs third-party `cargo-update`); upgrade-all skipped with a visible reason.
9. **Orphans**: pacman only (`-Qdtq`); detection never removes anything.
10. **No command palette / mouse yet.**

## Not yet done
pip/pipx/Snap, version-after in history, package collections, import/export, CLI.
