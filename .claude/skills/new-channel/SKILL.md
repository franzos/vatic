---
name: new-channel
description: Set up a new vatic channel — the TOML config that connects vatic to a messaging platform. Triggers on "new channel", "add channel", "configure channel", or "set up telegram/matrix/whatsapp/email".
---

# New Channel

Help them create a new vatic channel TOML file in `~/.config/vatic/channels/`. Channels are how vatic talks to the outside world — each one connects to a different messaging platform.

**Don't** tell them to go create files manually. Write the TOML yourself. Only pause when you need something from them — a token, credentials, that sort of thing. Use `AskUserQuestion` for all user-facing questions.

## Prerequisites

Make sure the channels directory exists:

```bash
mkdir -p ~/.config/vatic/channels
```

## 1. Choose Channel Type

AskUserQuestion: "Which channel would you like to set up?"

Options:
- **stdin (Recommended)** — Talk to vatic directly in the terminal. No external setup needed.
- **Telegram** — Connect vatic to a Telegram bot. Requires a bot token from @BotFather.
- **Matrix** — Connect vatic to a Matrix account. Requires a homeserver, username, and password.
- **WhatsApp** — Connect vatic to WhatsApp. Requires the `whatsapp` feature flag and nightly Rust.
- **Himalaya (Email)** — Have vatic check and respond to email. Requires `himalaya` CLI configured.

## 2. Configure by Type

### stdin

Write `~/.config/vatic/channels/stdin.toml`:

```toml
[channel]
type = "stdin"
```

That's it. It just reads from the terminal.

### Telegram

Ask: "Do you have a Telegram bot token? You can create one by messaging @BotFather on Telegram."

Once they've got a token, write `~/.config/vatic/channels/telegram.toml`:

```toml
[channel]
type = "telegram"
token = "<their-bot-token>"
```

The daemon uses long polling (`getUpdates`) — no webhook or public server needed.

### Matrix

Ask: "What's your Matrix homeserver URL, username, and password?"

Write `~/.config/vatic/channels/matrix.toml`:

```toml
[channel]
type = "matrix"
homeserver = "https://matrix.org"
user = "@vatic:matrix.org"
password = "<their-password>"
```

Session data gets persisted in `~/.local/share/vatic/channels/matrix/`. The daemon syncs via the Matrix client-server API — invite the bot to a room and it'll respond to messages matching job triggers.

Note: the password is stored in plaintext. It turns out that's fine for getting started, but for better security consider using the secrets proxy once it's supported in channel configs.

### WhatsApp

WhatsApp requires the `whatsapp` feature flag — needs nightly Rust due to `wacore-binary`. Build with: `cargo build --features whatsapp`

Write `~/.config/vatic/channels/whatsapp.toml`:

```toml
[channel]
type = "whatsapp"
```

Session data is stored in `~/.local/share/vatic/channels/whatsapp/whatsapp.db`. On first run, the daemon prints a QR code — scan it with WhatsApp to pair. After that it reconnects automatically.

### Himalaya (Email)

Ask: "Is `himalaya` installed and configured? Which account should vatic poll?"

Write `~/.config/vatic/channels/email.toml`:

```toml
[channel]
type = "himalaya"
poll_interval = 60
# account = "personal"  # optional — defaults to himalaya's default account
```

The daemon checks for new envelopes every `poll_interval` seconds and routes them to matching jobs.

## 3. Verify

List existing channels to confirm it's there:

```bash
ls ~/.config/vatic/channels/
```

Suggest running `vatic daemon` to test the channel, or offer to create a job that uses it (invoke the `new-job` skill).
