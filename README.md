# Dodo

Dodo is a local-first, markdown-based task manager for humans and AI agents.

## Why

- Plain text markdown files you own
- No lock-in and no cloud dependency required
- Works with any editor
- Agent-friendly format for automation and scripting

## Installation

### Cargo (placeholder)

```bash
cargo install dodo
```

### Manual binary install

```bash
git clone <repo-url>
cd dodo
cargo build --release
install -m 0755 target/release/dodo ~/.local/bin/dodo
```

Make sure `~/.local/bin` is in your `PATH`.

## Quick start

```bash
dodo add "Write project brief"
dodo add "Fix parser edge case" --file today --due 2026-03-30 --tag rust
dodo list --file today
```

## Commands

### Add

```bash
dodo add "Task description"
dodo add "Task description" --file today
dodo add "Task description" --file projects/roadmap --due 2026-04-01 --tag planning
```

### List

```bash
dodo list
dodo list --file today
dodo list --file projects
dodo list --file projects/roadmap
dodo list --file all
dodo list --tag rust
```

### Done

```bash
dodo done 1 --file today
dodo done 12 --file all
```

### Edit

```bash
dodo edit 1 --file inbox "Updated task text"
```

### Describe / Note

```bash
dodo describe 1 --file today "Blocked by API change"
dodo note 1 --file today "Blocked by API change"
```

### Delete

Permanently removes a task and its indented description lines. No confirmation prompt.

```bash
dodo delete 1 --file today
```

Output example:

```text
Deleted from today.md (#1): - [ ] Task text
```

### Clean

Removes all completed tasks (`- [x]`) and their description lines.

```bash
dodo clean --file today
dodo clean --file all
```

Output example:

```text
Cleaned today.md: removed 3 completed tasks
```

### Move

Moves a task from inbox to another file.

```bash
dodo move 2 today
dodo move 1 projects/roadmap
```

### Status

```bash
dodo status
```

### Overdue

```bash
dodo overdue
```

### Inbox

Shortcut for listing inbox tasks.

```bash
dodo inbox
```

### Today

Shortcut for listing today tasks.

```bash
dodo today
```

## File structure

Dodo stores everything in `~/.dodo/`:

```text
~/.dodo/
  inbox.md
  today.md
  waiting.md
  someday.md
  .current
  projects/
    <name>.md
  areas/
```

## Task format

Tasks are markdown checklist items:

```text
- [ ] Ship CLI delete command #rust 📅 2026-03-30
  Add integration tests for description-line deletion.
- [x] Publish 0.1.0 ✅ 2026-03-25
```

Conventions:

- `- [ ]` open task
- `- [x]` completed task
- `#tag` optional tag metadata
- `📅 YYYY-MM-DD` optional due date metadata
- `✅ YYYY-MM-DD` completion date metadata (added by `dodo done`)
- Indented lines immediately below a task are notes/descriptions

## License

MIT
