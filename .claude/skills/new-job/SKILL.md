---
name: new-job
description: Create a new vatic job — the TOML config that defines what the agent does, when, and where the output goes. Triggers on "new job", "add job", "create job", or "configure job".
---

# New Job

Help them create a new vatic job TOML file in `~/.config/vatic/jobs/`. Jobs are the core of vatic — they define what prompt to send, which agent runs it, in what environment, and where the result ends up.

**Don't** tell them to go create files manually. Write the TOML yourself. Only pause when you need a decision or some external info. Use `AskUserQuestion` for all user-facing questions.

## Prerequisites

Make sure the jobs directory exists:

```bash
mkdir -p ~/.config/vatic/jobs
```

## 1. What does the job do?

AskUserQuestion: "What should this job do? Describe it briefly."

Use their answer to figure out the agent prompt, environment, and output.

## 2. Name and Alias

AskUserQuestion: "What should the job be called? (e.g., 'Daily weather', 'Code reviewer')"

Derive a filename-safe alias from the name (lowercase, hyphens). Confirm the alias with them if it's ambiguous.

The alias is what they'll use for `vatic run <alias>`. The TOML file goes to `~/.config/vatic/jobs/<alias>.toml`.

## 3. Agent

AskUserQuestion: "Which AI backend?"

Options:
- **Claude CLI (Recommended)** — Uses local `claude` installation.
- **Ollama** — Local LLM via Ollama.

### Claude

```toml
[agent]
name = claude
```

If the job needs a system prompt — a persona, constraints, that sort of thing — add it:

```toml
[agent]
name = claude
prompt = "<system prompt>"
```

### Ollama

Ask: "Which model? (e.g., gemma3, llama3, mistral)"

```toml
[agent]
name = ollama
host = "http://localhost:11434"
model = "<model>"
```

## 4. Environment

AskUserQuestion: "Where should the agent run?"

Options:
- **local (Recommended)** — Runs directly on the system, no isolation.
- **guix-shell** — Runs inside `guix shell`. Can specify packages or use a `manifest.scm`.
- **guix-shell-container** — Like guix-shell but fully isolated (`--container --network`). Good for untrusted tasks.
- **podman** — Runs inside a Podman container. Auto-builds `vatic-agent` image on first use (Debian + Claude CLI).

### local

```toml
[environment]
name = local
```

### guix-shell / guix-shell-container

Ask: "Which packages does this job need? (e.g., `curl`, `node`). Leave empty to use the project's `manifest.scm`."

```toml
[environment]
name = "guix-shell"
packages = ["curl", "jq"]
```

Optionally set `pwd` to control the working directory.

### podman

```toml
[environment]
name = podman
```

Optionally specify a custom image — it defaults to `vatic-agent:latest`:

```toml
[environment]
name = podman
image = "node:22-slim"
pwd = "/home/franz/project"
```

## 5. Job Prompt

Write the `[job]` section based on what they described in step 1.

Available template tags — these get rendered before the prompt is sent to the agent:
- `{% date %}` — today's date (YYYY-MM-DD)
- `{% datetime %}` — current date and time
- `{% custom:key %}` — value from `dictionary.toml` (e.g., `{% custom:name %}`)
- `{% result %}` — output from the agent (used in output templates)
- `{% memory %}` — last memory entry's result
- `{% for i in memories limit:N %}...{% endfor %}` — iterate over past results
- `{% proxy:key %}` — secret from the secrets proxy

```toml
[job]
prompt = "<the prompt>"
```

If the job should run on a schedule, add a cron expression:

```toml
[job]
interval = "0 8 * * *"
prompt = "<the prompt>"
```

## 6. Input (Optional)

Ask: "Should this job be triggered by messages on a channel, or only run manually / on a schedule?"

If channel-triggered:

Ask: "Which channel? (e.g., stdin, telegram, matrix)"

For non-stdin channels, ask: "What trigger word should activate this job? (e.g., `/ask`, `weather`)" — without a trigger, the bot responds to everything, which is rarely what you want.

Then ask: "Where should the trigger match?" Options: **anywhere** (default), **start**, **end**.

```toml
[input]
channel = "<channel>"
trigger = "<trigger>"
trigger_match = "anywhere"   # "anywhere" (default), "start", or "end"
```

For stdin, omit trigger and trigger_match.

## 7. Session (Optional)

If the job is conversational — assistant-like, channel-triggered — ask: "How many previous messages should the agent see for context? (e.g., 20)"

```toml
[session]
context = 20
```

This gives the agent a sliding window of conversation history.

## 8. History (Optional)

If the job benefits from remembering past runs — email checkers, monitoring, anything that builds on previous results — ask: "Should vatic summarize each run for future reference?"

If yes, write a summarization prompt:

```toml
[history]
prompt = "Summarize the result of this interaction concisely."
```

The job prompt can then reference past results with `{% for i in memories %}`.

## 9. Output

AskUserQuestion: "Where should the result go?"

Options:
- **Desktop notification** — Shows a system notification via `notify-send`.
- **Channel** — Sends the result back to the input channel.
- **Email (msmtp)** — Sends an email via `msmtp`.
- **Command** — Runs a shell command with the result.

### notification

```toml
[output]
name = notification
message = "{% result %}"
```

### channel

```toml
[output]
channel = "<channel name>"
```

### msmtp

Ask: "Recipient email?" and "Subject line?"

```toml
[output]
name = msmtp
to = "<email>"
subject = "<subject>"
message = "{% result %}"
```

### command

Ask: "What command should run? Use `{% result %}` for the agent output."

```toml
[output]
name = command
command = "<their command>"
```

### Multiple outputs

Jobs can have more than one output. If they want multiple, use numbered sections:

```toml
[output]
name = notification
message = "{% result %}"

["output:1"]
name = msmtp
to = "user@example.com"
subject = "Report"
message = "{% result %}"
```

## 10. Write the File

Assemble all sections into a single TOML file and write it to `~/.config/vatic/jobs/<alias>.toml`.

## 11. Test

Run the job:

```bash
vatic run <alias>
```

If it works, let them know. If it fails, diagnose:
- `error: job not found` — check the alias matches the filename or the `alias` field
- Agent errors — check the agent is installed and authenticated
- Config parse errors — check TOML syntax
