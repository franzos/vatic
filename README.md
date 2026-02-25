# vatic

<p align="center">
  <img src="assets/logo.svg" alt="vatic" width="420">
</p>

I wanted a single binary that could run AI prompts on a schedule, respond to messages on Telegram or WhatsApp, and email me the results -- all configured with plain TOML files. No Python glue, no orchestration layer, no YAML indentation nightmares. Just a Rust daemon that reads a config directory and does the thing.

That's vatic. It's a TOML-configured AI agent framework. You define jobs that run prompts through LLM backends (Claude CLI, Ollama), on a cron schedule or triggered by channel messages, with templated prompts and multiple output targets.

## What it does

- **Sandboxed execution** -- Agents run in Podman containers or Guix shell (`--container --network`) with isolated filesystems. Credentials are mounted read-only, nothing else leaks. I've found this matters a lot when you're letting an LLM run shell commands on your behalf.
- **Channels** -- Telegram, Matrix, WhatsApp, email (via Himalaya), or plain stdin. The daemon listens on all configured channels and routes incoming messages to matching jobs.
- **Cron scheduling** -- Jobs run on cron expressions (`0 8 * * *`), on channel triggers, or both. Mix scheduled and interactive jobs freely.
- **Secrets proxy** -- API keys and tokens live in `secrets.toml`, referenced via `{% proxy:name %}` tags. Secrets stay out of prompts and job configs, which is the whole point.
- **Templated prompts** -- Date math, dictionary lookups, memory from previous runs, loops over collections, and pipe transforms -- all in a simple `{% tag %}` syntax. It's not Jinja, but it covers what I actually need.
- **Multiple outputs** -- Send results to desktop notifications, email, shell commands, or back to the channel. Stack multiple outputs per job.
- **Conversation memory** -- Sessions track message history for context-aware assistants. History summarization compresses past runs into memories so prompts don't bloat over time.
- **Flexible agents** -- Claude CLI or Ollama. Swap backends per job without changing anything else.

## Install

| Method | Command |
|--------|---------|
| Cargo | `cargo install vatic` |
| Debian/Ubuntu | Download [`.deb`](https://github.com/franzos/vatic/releases) -- `sudo dpkg -i vatic_*_amd64.deb` |
| Fedora/RHEL | Download [`.rpm`](https://github.com/franzos/vatic/releases) -- `sudo rpm -i vatic-*.x86_64.rpm` |
| Guix | `guix install -L <panther> vatic` ([Panther channel](https://github.com/franzos/panther)) |

Pre-built binaries for Linux (x86_64), macOS (Apple Silicon, Intel) on [GitHub Releases](https://github.com/franzos/vatic/releases).

## Quick start

Run `/setup` in Claude Code to get started -- it'll walk you through configuring your first channel and job. Use `/new-channel` or `/new-job` to add more later.

```bash
# Build
cargo build --release

# Run a job
vatic run weather

# List configured jobs
vatic list

# Start the daemon (channels + scheduled jobs)
vatic daemon
```

## Configuration

Everything lives under `~/.config/vatic/`:

```
~/.config/vatic/
  dictionary.toml          # variable substitution
  secrets.toml             # API keys for proxy tags
  jobs/*.toml              # job definitions
  channels/*.toml          # channel connections
```

Data goes to `~/.local/share/vatic/vatic.db` (SQLite).

### Job example

```toml
name = "Today's weather"
alias = "weather"

[agent]
name = claude

[job]
interval = "0 8 * * *"
prompt = "What's the expected weather for {% date %}, in Lisbon? Keep it to a 1-liner."

[environment]
name = local

[output]
name = notification
message = "Good morning {% custom:name %}; {% result %}"
```

### Agent backends

| Backend | Config | How it works |
|---------|--------|--------------|
| `claude` | `name = "claude"` | Spawns `claude --print` CLI |
| `ollama` | `name = "ollama"`, `host`, `model` | HTTP POST to `/api/generate` |

### Channels

| Channel | Config | How it works |
|---------|--------|--------------|
| `stdin` | `type = "stdin"` | Terminal I/O, good for getting started |
| `telegram` | `type = "telegram"`, `token` | Long polling via `getUpdates`, strips `@bot` mentions |
| `matrix` | `type = "matrix"`, `homeserver`, `user`, `password` | Sync loop via matrix-sdk, e2e encryption |
| `whatsapp` | `type = "whatsapp"` | QR pairing, feature-gated (`--features whatsapp`) |
| `himalaya` | `type = "himalaya"`, `poll_interval` | Polls email via `himalaya` CLI |

**Telegram in groups:** By default, Telegram bots have privacy mode enabled -- they only see messages that `@mention` the bot or start with `/`. Vatic automatically strips the `@botname` from incoming text so triggers match cleanly. If you want the bot to see *all* group messages (without requiring `@mention`), disable privacy mode via [@BotFather](https://t.me/BotFather): send `/setprivacy`, select your bot, choose `Disable`.

### Environments

| Environment | What it does |
|-------------|--------------|
| `local` | Runs commands directly |
| `guix-shell` | Wraps with `guix shell -m manifest.scm --` (or named packages) |
| `guix-shell-container` | Like `guix-shell` but isolated (`--container --network`), shares `~/.claude` |
| `podman` | Runs in a Podman container. Auto-builds `vatic-agent` image on first use |

The `guix-shell` and `guix-shell-container` environments accept a `packages` list. Without it, they fall back to `manifest.scm`:

```toml
[environment]
name = "guix-shell"
packages = ["curl", "jq"]

[environment]
name = "guix-shell-container"
packages = ["node", "rust"]
pwd = "/home/franz/project"
```

Podman accepts an optional `image` (defaults to `vatic-agent:latest`, built automatically):

```toml
[environment]
name = "podman"
image = "node:22-slim"   # optional
pwd = "/home/franz/project"
```

### Template tags

| Tag | Description |
|-----|-------------|
| `{% date %}` | Today's date (YYYY-MM-DD) |
| `{% date minus=1d %}` | Date offset (supports `d`, `h`, `m`) |
| `{% datetime %}` | Date and time |
| `{% datetimeiso %}` | ISO 8601 datetime |
| `{% custom:name %}` | Dictionary lookup |
| `{% result %}` | Job result (in output templates) |
| `{% message %}` | Incoming channel message |
| `{% sender %}` | Message sender identifier |
| `{% memory %}` | Last run result |
| `{% memory minus=2 %}` | Result from N runs ago |
| `{% proxy:name %}` | Secret proxy URL substitution |

Loops:

```
{% for i in memories limit:3 %}
Date: {% i.date %}
Result: {% i.result %}
{% endfor %}

{% for i in (1..3) %}
{% date minus=i"d" %}
{% endfor %}
```

Pipes: `{% i.result | summary %}` -- transforms the value through the agent.

### Outputs

| Output | Description |
|--------|-------------|
| `notification` | Desktop notification via `notify-send` |
| `msmtp` | Email via `msmtp` (requires `to`, optional `subject`) |
| `command` | Shell command execution |
| `channel` | Reply on the input channel |

Multiple outputs with `[output]` and `["output:1"]`.

### Channels + Sessions

Jobs can listen on channels and maintain conversation history:

```toml
[input]
channel = "telegram"
trigger = "vatic"
trigger_match = "anywhere"   # "anywhere" (default), "start", or "end"

[session]
context = 20

[output]
channel = "telegram"
```

### History summarization

Summarize results before storing them as memories -- useful when the raw output is too verbose to carry forward:

```toml
[history]
prompt = "Summarize: list the email IDs and key topics."
```

## Building on Guix

```bash
guix shell -m manifest.scm -- sh -c "CC=gcc cargo build --release"
```
