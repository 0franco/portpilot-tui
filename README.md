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
- **Tunnel Doctor** — preview generated commands and diagnose config, port, key, SSH, and kubectl failures
- **Persistent config** — tunnels survive restarts, stored as plain TOML you can commit to a repo
- **No daemon required** — PortPilot _is_ the process; quit the TUI and tunnels stop cleanly
- **Log pane** — tail of state-change events inline, full logs written to `~/.config/portpilot/logs/`

---

## Installation

### From source

```sh
git clone https://github.com/0franco/portpilot-tui.git
cd portpilot-tui
cargo install --path .
```

Or install directly from GitHub:

```sh
cargo install --git https://github.com/0franco/portpilot-tui.git
```

### Package managers

PortPilot is not published to crates.io or Homebrew yet, so these commands do not work yet:

```sh
cargo install portpilot
brew install portpilot
```

### Binary releases

Pre-built binaries are not published yet. When they are available, they will be attached to the [Releases](https://github.com/0franco/portpilot-tui/releases) page.

> **Requirements:** `ssh` must be on your `$PATH`. Key-based auth only — `BatchMode=yes` is set by design. No password prompts.

---

## Quick start

```sh
# Launch PortPilot
portpilot

# Press [n] to add your first tunnel, fill in the fields, press Enter to save.
# Press [Enter] or [Space] on a tunnel to start it.
# Press [D] to diagnose the selected tunnel before starting it.
# Press [?] for the full keybinding reference.
```

### Tunnel Doctor

Run preflight checks without starting a tunnel:

```sh
portpilot doctor mysql-over-bastion
portpilot doctor --project work mysql-over-bastion
portpilot doctor --no-remote mysql-over-bastion
```

Doctor reports the exact generated command, validates kind-specific config fields, checks local port availability, verifies identity files, and runs short SSH/kubectl reachability checks unless `--no-remote` is set.

---

## Keybindings

| Key | Action |
|---|---|
| `↑` / `↓` / `j` / `k` | Navigate tunnel list |
| `Enter` / `Space` | Toggle tunnel on/off |
| `D` | Run doctor checks for selected tunnel |
| `n` | New SSH tunnel |
| `N` | New Kubernetes tunnel |
| `K` | New Kubernetes via SSH tunnel |
| `B` | New Kubernetes via bastion tunnel |
| `e` | Edit selected tunnel |
| `d` / `Del` | Delete selected tunnel |
| `Tab` | Switch project |
| `?` | Help |
| `q` / `Ctrl-c` | Quit (stops all tunnels) |

---

## Config

Configs live at `~/.config/portpilot/projects/<name>.toml`. You can edit them by hand or use the TUI.

Each `.toml` file is one project in the UI. Press `Tab` to switch between them.

Example layout:

```toml
# ~/.config/portpilot/projects/prod.toml

[[tunnels]]
name          = "postgres-prod"
kind          = "ssh" # optional; omitted kind defaults to "ssh"
local_port    = 5432
remote_host   = "db.internal"
remote_port   = 5432
ssh_host      = "bastion.example.com"
ssh_user      = "alice"
auto_restart  = true
```

```toml
# ~/.config/portpilot/projects/staging.toml

[[tunnels]]
name          = "redis-staging"
kind          = "ssh"
local_port    = 6379
remote_host   = "redis.staging.internal"
remote_port   = 6379
ssh_host      = "bastion-staging.example.com"
identity_file = "~/.ssh/id_staging"
auto_restart  = false

[[tunnels]]
name          = "postgres-via-bastion"
kind          = "ssh"
local_port    = 15432
remote_host   = "aurora-pg.internal"
remote_port   = 5432
ssh_host      = "bastion-staging.example.com"
ssh_user      = "ec2-user"
identity_file = "~/.ssh/staging-bastion.pem"
auto_restart  = true
```

```toml
# ~/.config/portpilot/projects/k8s-staging.toml

[[tunnels]]
name          = "api-pod"
kind          = "kubernetes"
local_port    = 8080
remote_port   = 8080
target        = "svc/api"
namespace     = "staging"
context       = "staging-cluster"
auto_restart  = true

[[tunnels]]
name          = "mysql-over-ssh"
kind          = "kubernetes-via-ssh"
local_port    = 3306
remote_port   = 3306
ssh_host      = "k8s-admin.example.com"
ssh_user      = "ec2-user"
identity_file = "~/.ssh/k8s-admin.pem"
target        = "svc/mysql"
namespace     = "data"
remote_user   = "deploy" # optional: runs kubectl as this user on ssh_host
auto_restart  = true
```

```toml
# ~/.config/portpilot/projects/k8s-prod.toml

[[tunnels]]
name                   = "mysql-over-bastion"
kind                   = "kubernetes-via-bastion-ssh"
local_port             = 3306
remote_port            = 3306
bastion_host           = "bastion.example.com"
bastion_user           = "ec2-user"
bastion_identity_file  = "~/.ssh/bastion.pem"
target_host            = "10.0.10.25"
target_user            = "ec2-user"
target_identity_file   = "~/.ssh/k8s-target.pem"
target                 = "svc/mysql"
namespace              = "data"
target_remote_user     = "deploy" # optional: runs kubectl as this user on target_host
auto_restart           = true
```

For a simple database behind a bastion, use `kind = "ssh"`: `ssh_host` is the bastion you log into, and `remote_host` is the database host reachable from that bastion.

For `kubernetes-via-bastion-ssh`, `bastion_user` is the SSH login for `bastion_host`, `target_user` is the SSH login for `target_host`, and `target_remote_user` only controls the optional `sudo -u` user for the remote `kubectl` command.

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
