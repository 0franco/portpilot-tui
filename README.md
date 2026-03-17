# PortPilot ✈️

> Stop memorising `ssh -L 5432:db.internal:5432 -L 6379:redis:6379 user@bastion` forever.

**PortPilot** is a terminal UI for managing persistent SSH port-forwarding tunnels. Define named tunnels once, start and stop them with a keypress, and let PortPilot auto-restart them when they drop. Organise tunnels into project presets that match your workspaces.

---

## The problem

Every engineer who works with remote databases, internal services, or staging environments knows the ritual:

```sh
# production
ssh -L 5432:db.prod.internal:5432 -L 6379:redis.prod:6379 -N user@bastion-prod

# staging (different terminal)
ssh -L 5433:db.staging:5432 -L 6380:redis.staging:6379 -N user@bastion-staging
```

Then one of them drops silently. You wonder why Postgres won't connect. You grep your shell history. You paste the command into a new tab. Again. Every. Day.

PortPilot fixes this.

---

## Features

- **Named tunnels** — give each port-forward a human name, not a flag string
- **One-key toggle** — start and stop tunnels with `Enter` or `Space`
- **Auto-restart** — configurable per tunnel; backs off exponentially on repeated failures
- **Project presets** — switch between work/staging/personal configs with `Tab`
- **Live status** — see `UP`, `CONNECTING`, `FAILED`, `STOPPED` with process PID in real time
- **Persistent config** — tunnels survive restarts, stored as plain TOML you can commit to a repo
- **No daemon required** — PortPilot _is_ the process; quit the TUI and tunnels stop cleanly
- **Log pane** — tail of state-change events inline, full logs written to `~/.config/portpilot/logs/`

---

## Installation

### Homebrew (macOS)

```sh
brew install portpilot   # coming soon
```

### Cargo

```sh
cargo install portpilot
```

### Binary releases

Download a pre-built binary from the [Releases](https://github.com/yourusername/portpilot/releases) page.

> **Requirements:** `ssh` must be on your `$PATH`. Key-based auth only — `BatchMode=yes` is set by design. No password prompts.

---

## Quick start

```sh
# Launch PortPilot
portpilot

# Press [n] to add your first tunnel, fill in the fields, press Enter to save.
# Press [Enter] or [Space] on a tunnel to start it.
# Press [?] for the full keybinding reference.
```

---

## Keybindings

| Key | Action |
|---|---|
| `↑` / `↓` / `j` / `k` | Navigate tunnel list |
| `Enter` / `Space` | Toggle tunnel on/off |
| `n` | New tunnel |
| `e` | Edit selected tunnel |
| `d` / `Del` | Delete selected tunnel |
| `Tab` | Switch project |
| `?` | Help |
| `q` / `Ctrl-c` | Quit (stops all tunnels) |

---

## Config

Configs live at `~/.config/portpilot/projects/<name>.toml`. You can edit them by hand or use the TUI.

```toml
# ~/.config/portpilot/projects/config1.toml

[[tunnels]]
name          = "postgres-prod"
local_port    = 5432
remote_host   = "db.internal"
remote_port   = 5432
ssh_host      = "bastion.example.com"
ssh_user      = "alice"
auto_restart  = true

[[tunnels]]
name          = "redis-staging"
local_port    = 6379
remote_host   = "redis.staging.internal"
remote_port   = 6379
ssh_host      = "bastion-staging.example.com"
identity_file = "~/.ssh/id_staging"
auto_restart  = false
```

Multiple `.toml` files in `~/.config/portpilot/projects/` become separate projects, switchable with `Tab`.

---

## How it works

```
┌─────────────────────────────────────────────────────┐
│                   Ratatui TUI loop                   │
│   (single-threaded render + crossterm key events)    │
└────────────────────┬────────────────────────────────┘
                     │  mpsc::Sender<AppEvent>
          ┌──────────┴──────────┐
          │    TunnelManager    │
          │ (one task per live  │
          │      tunnel)        │
          └──────────┬──────────┘
                     │
        ┌────────────┴───────────┐
        │     TunnelWorker       │
        │  tokio::process::Child │  ← ssh -L …
        │  CancellationToken     │  ← clean shutdown
        │  exponential backoff   │  ← auto-restart
        └────────────────────────┘
```

---

## Platform support

| Platform | Status    |
|---|-----------|
| Linux | ✅         |
| macOS | ✅         |
| Windows | ❌ Not yet |

---

## Contributing

PRs welcome!

---

## License

MIT
