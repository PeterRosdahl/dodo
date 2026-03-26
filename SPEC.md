# Dodo Task Format (DTF) v1 Specification

## 1. Overview
Dodo Task Format (DTF) v1 is a plain-text task format designed to be readable by humans and parseable by AI agents.

DTF v1 is built on GitHub Flavored Markdown task list items (`- [ ]` and `- [x]`) and extends them with emoji-prefixed metadata tokens, tags, and optional description blocks.

Goals:
- Keep task files simple and editable in any text editor.
- Preserve deterministic parsing for tools and agents.
- Support stable task identity and automation workflows.

## 2. Task Line Syntax
A task line MUST use one of these forms:

```md
- [ ] Task description [metadata tokens]
- [x] Completed task [metadata tokens]
```

Parsing rules:
- A task line starts with `- [ ] ` (open) or `- [x] ` (completed).
- The first non-checkbox text is the task description.
- Metadata tokens MAY appear anywhere after the description and are order-independent.
- Unknown tokens SHOULD be preserved as plain text.

## 3. Metadata Tokens
Metadata tokens are emoji-prefixed fields embedded in task lines. Tokens are order-independent.

| Token | Syntax | Example | Description |
|-------|--------|---------|-------------|
| 📅 | `📅 YYYY-MM-DD` | `📅 2026-03-27` | Due date |
| ⏰ | `⏰ HH:MM` or `⏰ YYYY-MM-DDTHH:MM` | `⏰ 09:00` | Alarm/reminder time |
| 📍 | `📍 Location name` | `📍 Hemköp Kungsholmen` | Physical location context |
| 👤 | `👤 name` | `👤 henrik` | Assigned to person or agent |
| 🔁 | `🔁 freq` | `🔁 weekly` | Recurrence (`daily`/`weekly`/`monthly`/`YYYY-MM-DD`) |
| 🆔 | `🆔 6hex` | `🆔 a3f9b2` | Stable unique task ID |
| ⚡ | `⚡ level` | `⚡ high` | Priority (`high`/`medium`/`low`) |
| 🔗 | `🔗 url` | `🔗 https://example.com/spec` | Reference URL |
| 📎 | `📎 path` | `📎 ~/docs/contract.pdf` | File attachment path |
| ✅ | `✅ YYYY-MM-DD` | `✅ 2026-03-26` | Completion date (added by `dodo done`) |

Token parsing rules:
- Tokens MAY appear in any order.
- A token value extends from token marker to the next recognized token marker or end of line.
- Last occurrence of the same token on a line SHOULD win unless implementation explicitly supports arrays.
- `🆔` values MUST match `^[a-f0-9]{6}$`.
- Date values MUST use ISO local date format `YYYY-MM-DD`.
- Time-only alarm values MUST use 24-hour `HH:MM`.
- Datetime alarm values MUST use local `YYYY-MM-DDTHH:MM`.
- `✅` SHOULD only appear on completed tasks.

## 4. Tags
Tags are inline markers with syntax `#tagname` and MAY appear anywhere in task text.

Rules:
- Multiple tags per task are allowed.
- Tags are used for filtering and cross-list membership.
- Tags SHOULD be case-insensitive in matching, while preserving original casing in storage.

Examples:
- `- [ ] Prepare monthly report #work #finance 📅 2026-03-31`
- `- [x] Morning run #health ✅ 2026-03-26`

## 5. Description Blocks
Indented lines immediately following a task line belong to that task as description/notes.

Rules:
- A description block line MUST start with two spaces.
- The block continues while subsequent lines remain indented by two spaces.
- A blank line ends the description block.
- A non-indented line starts a new top-level element.

Example:

```md
- [ ] Call supplier 📍 Office 🆔 c41e9d
  Ask for Q2 lead times.
  Confirm updated payment terms.

- [ ] Next task
```

## 6. File Structure
DTF v1 files are stored under `~/.dodo/` with this layout:

```text
~/.dodo/
  inbox.md
  today.md
  waiting.md
  someday.md
  projects/
    <name>.md
  areas/
    <name>.md
  templates/
    <name>.md
  config.toml
  .dodo-state
```

Notes:
- `.dodo-state` is internal state and SHOULD be gitignored.
- `projects/` stores project-scoped task lists.
- `areas/` stores long-lived responsibility domains.
- `templates/` stores reusable task templates.

## 7. YAML Frontmatter
A DTF file MAY include optional YAML frontmatter at the top:

```md
---
title: Today
owner: henrik
timezone: Europe/Stockholm
---
```

Rules:
- Frontmatter MUST be the first block in the file.
- It MUST be enclosed by `---` delimiters.
- Parsers MUST ignore frontmatter when scanning task lines.

## 8. Full Example
Example `today.md`:

```md
---
title: Today
owner: henrik
date: 2026-03-26
timezone: Europe/Stockholm
---

- [ ] Buy groceries #errands 📅 2026-03-27 ⏰ 17:30 📍 Hemköp Kungsholmen ⚡ medium 🆔 a3f9b2
  Milk, eggs, coffee.
  Check weekend discounts in app.

- [ ] Review contract draft #legal #work 👤 henrik 🔗 https://example.com/contracts/msa-v4 📎 ~/docs/contract.pdf ⚡ high 🆔 e8d1c0
  Focus on liability and termination clauses.

- [ ] Team standup prep #work 🔁 daily ⏰ 09:00 👤 agent 🆔 4bc122

- [x] Send invoice #finance ✅ 2026-03-26 🆔 90af11
  Sent via accounting portal.

- [ ] Follow up with vendor #waiting 📅 2026-03-29 👤 henrik 🆔 b72e44
```

## 9. Agent Integration
Agent behavior requirements:
- Always use `--json` for reads/parsing to avoid brittle text parsing.
- Use `🆔` IDs for stable references across edits and reordering.
- Use webhook/exec alarm handlers for `⏰` reminders.
- Use `dodo inbox add` when creating new tasks.
- Never modify task files directly; use CLI commands to preserve format integrity.

Recommended workflow:
1. Read task data via CLI JSON output.
2. Resolve target task by `🆔`.
3. Apply updates through CLI subcommands.
4. Re-read and verify resulting state.

## 10. Versioning
- This document defines **DTF v1**.
- DTF uses semantic versioning for spec evolution.
- v1.x updates MUST remain backward compatible with valid v1 documents.
- Breaking format changes require a major version increment (v2+).

---

DTF v1 aims to keep plain-text task management durable, automation-friendly, and easy to adopt in both human and agent workflows.
