# Dodo — Design Specification

## Overview

Dodo is a local-first task manager for humans and AI agents. Tasks are stored as plain markdown files on device. The app is a UI layer on top of these files — it reads and writes the same format as the CLI.

---

## Core Concepts (must be reflected in design)

- **Files are truth.** Tasks live in files: inbox.md, today.md, waiting.md, someday.md, projects/, areas/
- **Agents can act on tasks.** An AI agent may add, complete, or log progress on tasks. The UI should make agent activity visible.
- **Sections group tasks within a file.** Files can have `## Section Name` headers.
- **Sub-tasks nest under tasks.** A task can have child tasks with a progress indicator (2/3).
- **Metadata is invisible by default.** Due dates, tags, priorities etc. are parsed from the file but shown cleanly — not as raw emoji syntax.

---

## Screens & Views

### 1. Inbox
- List of all uncategorized/unprocessed tasks
- Quick add input (supports natural language: "Call Henrik tomorrow at 14:00")
- Empty state: inbox zero message
- Each task shows: text, due date (if set), tags, priority indicator
- Tap/click to expand: show description, sub-tasks, log entries, metadata
- Swipe or action menu: mark done, move to today, move to project, delete

### 2. Today
- Tasks due today + tasks manually moved to today
- Same layout as Inbox
- Overdue tasks appear at top with red indicator
- Progress summary: "3 of 7 done today"

### 3. Upcoming
- Tasks with due dates in the next 7–30 days (configurable)
- Grouped by date: "Today", "Tomorrow", "Mon 31 mar", etc.
- Shows which file/project each task belongs to

### 4. Projects
- List of all project files (projects/)
- Each project shows: name, open task count, last activity
- Tap to open project view (same layout as Inbox but scoped to that file)
- Create new project
- Archive/delete project

### 5. Areas
- Same as Projects but for areas/ directory
- Areas are ongoing responsibilities, not time-bound projects

### 6. Search
- Full-text search across all files
- Filter by: tag, assigned, priority, due date range, file
- Results grouped by file
- Highlight matching text

### 7. Someday / Later
- Simple list view of someday.md
- No due dates required
- Quick capture for ideas/future tasks

### 8. Waiting
- Tasks waiting on someone/something
- Shows `👤 person` assignment if set
- Overdue waiting items highlighted (waiting > 3 days without update)

---

## Task Detail View

Opened when tapping a task. Shows and allows editing of:

- Task text (editable inline)
- Checkbox (done/undone toggle)
- Due date picker
- Alarm/reminder time picker
- Priority selector (high / medium / low)
- Location field (📍)
- Assigned to field (👤 person or agent name)
- Recurrence selector (daily / weekly / monthly / weekdays / none)
- Tags (add/remove #tags)
- Move to file (inbox / today / waiting / someday / project / area)
- Reference URL (🔗)
- File attachment (📎)

### Sub-tasks panel
- List of sub-tasks with checkboxes
- Add sub-task input
- Progress bar or ratio (2/3 done)

### Description / Notes panel
- Free-form text below task
- Multi-line, supports markdown

### Activity / Log panel
- Timestamped log entries (added by user or agent)
- "> 2026-03-26 21:00 · Sent email to Henrik"
- Add log entry input
- Read-only entries from agents are visually distinct

---

## Quick Add

Available globally (keyboard shortcut, floating button, or widget):
- Single text input with natural language parsing
- Auto-detects: dates ("imorgon", "tomorrow", "kl 14"), priority ("hög prio"), recurrence ("varje måndag"), tags (#project)
- Shows parsed metadata as preview chips before saving
- Destination selector: inbox / today / project
- Save button + keyboard submit

---

## Filters & Smart Views

User-configurable saved filters. Each filter is a named view:
- Filter criteria: due date range, tag, assigned, priority, file, overdue
- Multiple criteria combined (AND logic)
- Saved filters appear in sidebar/navigation alongside default views

---

## Status Dashboard

Overview screen (or sidebar widget):
- Total open tasks
- Overdue count (with warning if > 0)
- Today's count
- Tasks assigned to agents
- Recent agent activity (last 3 log entries from agents)
- Stale inbox warning (tasks > 3 days without action)

---

## Notifications & Alarms

- Tasks with ⏰ time trigger local notifications
- Notification shows task text + actions: "Done" / "Snooze 1h" / "View"
- Location-based reminders (📍) when near a specified location
- Overdue summary notification (daily digest option)

---

## Multiplayer / Shared Lists (Sync tier)

When sync is enabled:
- Tasks can be assigned to other users (`👤 username`)
- Shared project files visible to all collaborators
- Real-time updates when others complete/add tasks
- Presence indicators (who's looking at the same project)
- Comment threads on tasks (separate from log entries)
- Conflict resolution UI (if two people edit same task offline)

---

## Agent Activity View

Dedicated view showing what AI agents have done recently:
- Tasks added by agents (with agent name/icon)
- Tasks completed by agents
- Log entries from agents
- Tasks currently assigned to agents (`👤 puck`, `👤 assistant`)
- Option to review + confirm agent actions before they take effect (optional trust mode)

---

## Settings

### General
- Default file for quick add (inbox / today)
- Week start day (Monday / Sunday)
- Date format (Swedish / ISO / relative)
- Language (Swedish / English)

### Files & Storage
- Show current storage location (~/.dodo/)
- Open in file manager / Finder
- Git integration toggle (auto-commit on change)

### Sync (if enabled)
- Account management
- Connected devices list
- Conflict resolution preference

### Notifications
- Enable/disable alarms
- Overdue digest: off / daily / immediate
- Agent activity notifications: off / summary / immediate

### Integrations
- Webhook URL for dodo watch
- Krisp / Granola integration (webhook endpoint)
- MCP server toggle (for AI assistant access)
- Connected agents list

### Templates
- List of templates in ~/.dodo/templates/
- Create / edit / delete templates
- Apply template to create new project/file

---

## Navigation Structure

```
Sidebar / Tab bar:
├── Inbox           (badge: unread count)
├── Today           (badge: due today count)
├── Upcoming
├── Projects
│   ├── Project A
│   └── Project B
├── Areas
├── Waiting
├── Someday
├── Saved Filters   (user-defined)
└── Agent Activity
```

---

## Platform Notes

### Mobile (iOS / Android)
- Bottom tab navigation
- Swipe gestures on task rows (done, move, delete)
- Widget: today's tasks count + quick add
- Share sheet integration (capture links as tasks)
- Siri / Google Assistant shortcut for quick add

### Desktop (macOS / Windows / Linux)
- Sidebar navigation
- Keyboard shortcuts for all common actions
- Command palette (Cmd+K) for quick navigation and actions
- Menu bar / system tray: quick add, today count
- Multi-window support (open two projects side by side)

### CLI (already built)
- All CLI commands remain fully functional
- App and CLI share the same ~/.dodo/ files
- No sync required — same local directory

---

## Accessibility

- Full keyboard navigation
- Screen reader labels on all interactive elements
- High contrast mode support
- Minimum touch target size on mobile
- No information conveyed by color alone (always paired with icon/text)

---

## Edge Cases to Design For

- Empty states for every view (no tasks, no projects, etc.)
- Offline state (sync unavailable — show last synced time)
- Conflict state (two versions of same task)
- Large task counts (100+ tasks in one file — virtual scrolling)
- Long task text (truncation + expand)
- Tasks with many sub-tasks (collapsed by default)
- Agent-only tasks (no human interaction expected)
- Recurring tasks that have just been regenerated
- File parse error (corrupted markdown — graceful fallback)
