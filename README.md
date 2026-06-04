# pelper

A terminal DX helper that brings your everyday dev tools into one polished TUI.
Each tool is a self-contained **feature**; **Projects** — a visual overview of
your git repositories with update and prune actions — is the first of many.

```
 Projects                                    ⠹ scanning… (12)
 ~/coding/web-app
─────────────────────────────────────────────────────────────
    PROJECT             BRANCH            SYNC      LAST
 ●  web-app             feat/new-nav      —         1h ago
    api-gateway         main              ✓         8h ago
    cli-tools           main              ✓         2d ago
 ↑/↓ move  ⏎ open  u update  p prune  r rescan  ? help  q quit
```

## Features

The **Projects** feature scans a set of root directories for git repositories
and lets you:

- **Dashboard** — list every project with its current branch, dirty state,
  ahead/behind counts and last-commit time (`2d ago`), most-recently-touched
  first. The scan runs in parallel and streams in.
- **Viewer** (project detail) — list a project's local branches, with the
  default branch (`main`/`master`) on top and the rest by latest change, showing
  the author, last-commit time, sync status and the selected branch's subject.
- **Updater** — fetch + prune each project and fast-forward its default branch
  (`pull --ff-only`), automatically stashing/restoring local work and returning
  to the original branch. Runs across all projects or just one, with live
  progress.
- **Pruner** — find local branches whose upstream is **gone** (deleted on the
  remote ⇒ merged, including squash-merges), review them in a checklist, then
  delete with a two-step confirmation. The deleted tip SHA is shown so it stays
  recoverable via the reflog.

## Install

Requirements: a recent Rust toolchain and `git` on your `PATH`.

```sh
cargo build --release
./target/release/pelper
# or put it on your PATH:
cargo install --path .
```

## Usage

Run with no arguments for the interactive TUI:

```sh
pelper
```

Or use the headless commands (same engine, scriptable):

```sh
pelper scan                              # list discovered projects
pelper update                            # fast-forward every project's default branch
pelper update --project web-app          # …or just one
pelper prune                             # DRY-RUN: list merged (gone) branches
pelper prune --yes                       # actually delete them
pelper prune --project web-app --yes
```

`prune` never deletes without `--yes`.

## Keybindings

Press `?` on any screen for a context-specific overlay.

| Screen      | Keys                                                                          |
| ----------- | ----------------------------------------------------------------------------- |
| Home        | `↑/↓`·`j/k` move · `⏎` open · `?` help · `q` quit                              |
| Dashboard   | `↑/↓` move · `⏎` detail · `u` update all · `p` prune all · `r` rescan · `Esc` home · `q` quit |
| Detail      | `↑/↓` move · `u` update · `p` prune · `Esc` back · `q` quit                    |
| Update      | `↑/↓` move · `Esc` back · `q` quit                                             |
| Prune       | `↑/↓` move · `Space` toggle · `a` all · `d` delete · `y/n` confirm · `Esc` back |

## Configuration

On first run pelper writes a starter config to
`~/.config/pelper/config.toml` (or `$XDG_CONFIG_HOME/pelper/config.toml`):

```toml
roots = ["~/coding"]               # directories whose git subdirs are projects
default_branches = ["main", "master"]  # treated as a project's "main", in priority order
```

`roots` accepts `~` and multiple entries.

## Safety

pelper shells out to your system `git`, so it uses your existing config and
credentials. The mutating actions are conservative:

- **Update** only ever fast-forwards (`--ff-only`); a diverged branch is reported
  and left untouched. Local changes are auto-stashed and restored — if the
  restore conflicts, your changes are left safe in `git stash` and flagged.
- **Prune** only considers branches whose upstream is gone, never the current or
  default branch, requires explicit confirmation, and uses `git branch -D` (the
  reflog keeps the tip recoverable; the SHA is shown).

## Architecture

```
src/
  main.rs        entry point + clap subcommands (TUI when none given)
  config.rs      config load / starter file
  cli.rs         headless scan / update / prune
  git/           core git logic — no TUI dependencies, unit + integration tested
    scan.rs      discover repos under roots (parallel)
    repo.rs      per-repo snapshot (branch, dirty, ahead/behind, last commit)
    branches.rs  per-branch detail (author, subject, upstream, gone, HEAD)
    update.rs    fetch + ff-only update with stash/checkout dance
    prune.rs     gone-branch detection + delete
    time.rs      relative "2d ago" formatting
  tui/           ratatui front-end
    app.rs       app state, event loop, screen router
    home.rs      feature launcher
    projects.rs  dashboard
    detail.rs    project detail / branch viewer
    update.rs    update progress screen
    prune.rs     prune checklist screen
    help.rs      keybindings overlay
```

The `git/` layer is pure and TUI-free, so it backs both the TUI and the headless
CLI and is covered by tests (including real-git integration tests for update and
prune).

### Adding a feature

1. Add an entry to `tui::home::features()`.
2. Add a `Screen` variant in `tui::app` and route it in `on_key` / `draw`.
3. Implement the screen module (and any pure logic under `git/` or a new module).

## Development

```sh
cargo build
cargo test     # unit + integration tests
cargo clippy
cargo run      # launch the TUI
```
