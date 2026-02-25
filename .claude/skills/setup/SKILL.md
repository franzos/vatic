---
name: setup
description: Walk them through first-time vatic setup — channels, jobs, the whole thing. Triggers on "setup", "get started", "configure vatic", or first-time setup requests.
---

# Vatic Setup

Help them get vatic running from scratch. Explain what channels and jobs are, then set up a real one of each.

**The thing is** — don't tell them to go create files manually. Write the TOML files yourself. Only pause when you actually need something from them (picking a channel type, handing over credentials). Use `AskUserQuestion` for all user-facing questions.

## 1. Explain Concepts

Before asking anything, give them the lay of the land:

**Channels** are connections to messaging platforms — Telegram, Matrix, WhatsApp, email, or just the terminal. When the daemon runs, it listens on all configured channels for incoming messages. Each channel is a TOML file in `~/.config/vatic/channels/`. Supported types: `stdin` (terminal I/O), `telegram`, `matrix`, `whatsapp` (feature-gated), and `himalaya` (email via CLI).

**Jobs** are the actual tasks vatic runs. A job defines which AI agent to use, what prompt to send, where to run it, and where the output goes. You can trigger jobs manually (`vatic run <alias>`), on a cron schedule, or by messages arriving on a channel. Each job lives as its own TOML file in `~/.config/vatic/jobs/`.

The typical flow: message arrives on a channel -> vatic finds matching jobs -> renders the prompt template -> runs the agent -> sends the result to the configured output.

## 2. Create Config Directories

Make sure the config and data directories exist:

```bash
mkdir -p ~/.config/vatic/channels ~/.config/vatic/jobs
mkdir -p ~/.local/share/vatic
```

## 3. Dictionary (Optional)

Ask: "What's your name? This is used for personalized prompts like `{% custom:name %}`."

Write `~/.config/vatic/dictionary.toml` — this is where vatic stores personal details that templates can reference:

```toml
[general]
name = "<their name>"
```

## 4. Choose a Channel

AskUserQuestion: "Which channel would you like to set up?"

Options:
- **stdin (Recommended)** — Talk to vatic directly in the terminal. No external setup needed. Good for getting started.
- **Telegram** — Connect vatic to a Telegram bot. Requires a bot token from @BotFather.
- **Matrix** — Connect vatic to a Matrix account. Requires a homeserver, username, and password.
- **WhatsApp** — Connect vatic to WhatsApp. Requires the `whatsapp` feature flag and nightly Rust.
- **Himalaya (Email)** — Have vatic check and respond to email. Requires `himalaya` CLI configured.

### 4a. stdin

Write `~/.config/vatic/channels/stdin.toml`:

```toml
[channel]
type = "stdin"
```

No further setup needed. It just reads from the terminal.

### 4b. Telegram

Ask: "Do you have a Telegram bot token? You can create one by messaging @BotFather on Telegram."

Once they have a token, write `~/.config/vatic/channels/telegram.toml`:

```toml
[channel]
type = "telegram"
token = "<their-bot-token>"
```

The daemon uses long polling (`getUpdates`) — no webhook or public server needed. Messages sent to the bot show up as channel input to matching jobs.

### 4c. Matrix

Ask: "What's your Matrix homeserver URL, username, and password?"

Write `~/.config/vatic/channels/matrix.toml`:

```toml
[channel]
type = "matrix"
homeserver = "https://matrix.org"
user = "@vatic:matrix.org"
password = "<their-password>"
```

Session data gets persisted in `~/.local/share/vatic/channels/matrix/`. The daemon syncs continuously via the Matrix client-server API. Invite the bot to a room and it'll respond to messages matching job triggers.

Note: the password is stored in plaintext in the TOML file. I've found that's fine for getting started, but for better security consider using the secrets proxy (`{% proxy:matrix_password %}`) once that's supported in channel configs.

### 4d. WhatsApp

WhatsApp requires the `whatsapp` feature flag — it needs nightly Rust due to `wacore-binary`. Build with: `cargo build --features whatsapp`

Write `~/.config/vatic/channels/whatsapp.toml`:

```toml
[channel]
type = "whatsapp"
```

Session data is stored in `~/.local/share/vatic/channels/whatsapp/whatsapp.db`. On first run, the daemon prints a QR code to the terminal — scan it with WhatsApp to pair. After that it auto-reconnects from the saved session.

### 4e. Himalaya (Email)

Requires `himalaya` CLI installed and configured. Ask: "Is `himalaya` installed and configured? Which account should vatic poll?"

Write `~/.config/vatic/channels/email.toml`:

```toml
[channel]
type = "himalaya"
poll_interval = 60
# account = "personal"  # optional — defaults to himalaya's default account
```

The daemon polls for new envelopes every `poll_interval` seconds, reads them, and routes them as channel input to matching jobs.

## 5. Configure a Job

AskUserQuestion: "What kind of job would you like to create?"

Options:
- **Weather briefing** — Get a daily weather summary for your city.
- **Personal assistant** — A conversational assistant on your channel.
- **Email checker** — Check email and summarize what's new (requires `himalaya`).
- **Custom** — Describe what you want and I'll write the job config.

**Important:** For channel-triggered jobs (5a channel variant, 5b), remember which channel they set up in step 4. Use that channel name in `[input]` and `[output]`. For non-stdin channels (telegram, matrix, whatsapp), always ask for a trigger word — otherwise the bot responds to every single message.

AskUserQuestion (if channel is not stdin): "What trigger word should the bot respond to? For example, `/ask` or `vatic`. The bot will only respond to messages containing this word."

AskUserQuestion (if trigger provided): "Where should the trigger match?"

Options:
- **anywhere (Recommended)** — Matches if the trigger appears anywhere in the message.
- **start** — Only matches at the beginning of the message.
- **end** — Only matches at the end of the message.

### 5a. Weather

Ask: "Which city should I check weather for?"

Write `~/.config/vatic/jobs/weather.toml`:

```toml
name = "Today's weather"
alias = "weather"

[agent]
name = claude

[job]
prompt = "What's the expected weather for {% date %}, in <city>? Keep it to a 1-liner."

[environment]
name = local

[output]
name = notification
message = "Good morning {% custom:name %}; {% result %}"
```

Tell them they can test it: `vatic run weather`

If they set up a channel, also offer a channel-triggered version. Use the channel they chose in step 4 and include their trigger word:

```toml
[input]
channel = "<their-channel>"
trigger = "<their-trigger>"
trigger_match = "<their-match>"   # "anywhere" (default), "start", or "end"

[output]
channel = "<their-channel>"
```

### 5b. Personal assistant

Ask for a trigger word if the channel isn't stdin. For stdin, no trigger is needed — they're typing directly.

Write `~/.config/vatic/jobs/assistant.toml` using the channel from step 4:

```toml
name = "Personal Assistant"
alias = "assistant"

[agent]
name = claude
prompt = "You are a helpful personal assistant for {% custom:name %}."

[job]
# Prompt comes from the channel message

[session]
context = 20

[environment]
name = local

[input]
channel = "<their-channel>"
trigger = "<their-trigger>"         # omit for stdin
trigger_match = "<their-match>"   # "anywhere" (default), "start", or "end"

[output]
channel = "<their-channel>"
```

Tell them: "Run `vatic daemon` and start chatting. The assistant remembers the last 20 messages."

### 5c. Email checker

Ask: "Is `himalaya` installed and configured?" If not, explain: "This job uses the `himalaya` CLI to check your email. Install it first, then come back."

If yes, write `~/.config/vatic/jobs/email.toml`:

```toml
name = "Email review"
alias = "email"

[agent]
name = claude
prompt = "You are a personal assistant."

[job]
prompt = """Check my email using Himalaya CLI and tell me if there's something new.
- List email: `himalaya`
- Open message: `himalaya message read <id>`

Previous results:
{% for i in memories limit:7 %}
Date: {% i.date %}
Result: {% i.result %}
{% endfor %}
"""

[history]
prompt = """Summarize the result of this interaction. For example:
New email IDs: 74, 75, 76, 78
Topics: invoice from X, meeting reminder, newsletter"""

[environment]
name = local

[output]
name = notification
```

Tell them: `vatic run email`

### 5d. Custom

Ask them to describe what they want. Then:

1. Pick the right agent (`claude` for CLI tasks, suggest `ollama` if they want local/offline)
2. Pick the environment:
   - `local` — runs directly, no isolation
   - `guix-shell` — runs inside `guix shell`, uses `manifest.scm` or named `packages`
   - `guix-shell-container` — like `guix-shell` but fully isolated (`--container --network`), good for untrusted tasks
   - `podman` — runs in a Podman container (auto-builds `vatic-agent` image on first use with Claude pre-installed). Accepts optional `image` override.
3. If they pick `guix-shell` or `guix-shell-container`, ask: "Which packages does this job need? (e.g., `curl`, `node`). Leave empty to use the project's `manifest.scm`."
4. Write the prompt template using available tags
5. Choose an output (notification, email, command, or channel)
6. Write the TOML file
7. Offer to test with `vatic run <alias>`

## 6. Choose an Agent Backend

AskUserQuestion: "Which AI backend do you want to use?"

Options:
- **Claude CLI (Recommended)** — Uses your local `claude` installation. Requires Claude CLI installed and authenticated.
- **Ollama** — Local LLM via Ollama. Requires Ollama running at localhost:11434.

### 6a. Claude

Verify: `which claude`. If it's missing, tell them to install it: `curl -fsSL https://claude.ai/install.sh | bash`

### 6b. Ollama

Ask: "Which model? (e.g., gemma3, llama3, mistral)"

Verify: `curl -s http://localhost:11434/api/tags`. If connection refused, tell them to start Ollama.

Update the job's `[agent]` section:

```toml
[agent]
name = ollama
host = "http://localhost:11434"
model = "<chosen model>"
```

## 7. Test

Run the job they just created:

```bash
vatic run <alias>
```

If it works, let them know. If it fails, diagnose:

- `error: job not found` — check the alias matches the filename or the `alias` field
- `error: claude` / agent errors — check the agent is installed and authenticated
- Config parse errors — check TOML syntax

If they set up a channel job, also test the daemon:

```bash
vatic daemon
```

Then type a message and verify a response comes back.

## 8. Next Steps

Suggest:
- Add a cron schedule: `interval = "0 8 * * *"` in the `[job]` section
- Add email output: set `[output] name = "msmtp"` with `to` and `subject`
- Add memories: use `{% memory %}` or `{% for i in memories %}` to reference previous runs
- Create more jobs in `~/.config/vatic/jobs/`
