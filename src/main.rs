use std::collections::HashSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

const C_RESET: &str = "\x1b[0m";
const C_BOLD: &str = "\x1b[1m";
const C_GREEN: &str = "\x1b[32m";
const C_YELLOW: &str = "\x1b[33m";
const C_BLUE: &str = "\x1b[34m";
const C_RED: &str = "\x1b[31m";
const C_CYAN: &str = "\x1b[36m";
const C_DIM: &str = "\x1b[2m";

enum ListSelection {
    Inbox,
    Today,
    Projects,
    Project(String),
    All,
    Tag(String),
}

#[derive(Clone, Copy)]
enum CoreFile {
    Inbox,
    Today,
    Waiting,
    Someday,
}

#[derive(Clone)]
enum TaskFile {
    Core(CoreFile),
    Project(String),
}

enum DoneSelection {
    File(TaskFile),
    All,
}

struct ParsedTaskLine {
    done: bool,
    text: String,
    tags: Vec<String>,
    due: Option<String>,
    id: Option<String>,
    recurrence: Option<Recurrence>,
    done_at: Option<String>,
}

struct DisplayTaskLine {
    done: bool,
    text: String,
    tags: Vec<String>,
    due: Option<String>,
    alarm: Option<AlarmSpec>,
    done_at: Option<String>,
}

#[derive(Clone)]
enum AlarmSpec {
    Time(String),
    DateTime(String),
}

struct AlarmRecord {
    when: String,
    file: TaskFile,
    line: String,
    descriptions: Vec<String>,
}

struct WatchConfig {
    handler: String,
    interval: u64,
}

struct AppConfig {
    watch: WatchConfig,
}

enum ConfigValue {
    Text(String),
    Number(u64),
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Recurrence {
    Daily,
    Weekly,
    Monthly,
    Weekdays,
}

struct TaskJsonRecord {
    id: Option<String>,
    done: bool,
    text: String,
    tags: Vec<String>,
    due: Option<String>,
    file: String,
    description: Vec<String>,
    subtasks: Vec<SubtaskJsonRecord>,
}

struct SubtaskJsonRecord {
    done: bool,
    text: String,
    done_at: Option<String>,
}

struct SubtaskLine {
    done: bool,
    text: String,
    done_at: Option<String>,
}

impl CoreFile {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "inbox" => Some(Self::Inbox),
            "today" => Some(Self::Today),
            "waiting" => Some(Self::Waiting),
            "someday" => Some(Self::Someday),
            _ => None,
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Inbox => "inbox.md",
            Self::Today => "today.md",
            Self::Waiting => "waiting.md",
            Self::Someday => "someday.md",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Today => "Today",
            Self::Waiting => "Waiting",
            Self::Someday => "Someday",
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{C_RED}Error:{C_RESET} {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let app = bootstrap_paths().map_err(|e| format!("failed to initialize ~/.dodo: {e}"))?;
    let mut args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        print_help();
        return Ok(());
    }

    let cmd = args.remove(0);
    match cmd.as_str() {
        "add" => cmd_add(&app, &args),
        "list" => cmd_list(&app, &args),
        "done" => cmd_done(&app, &args),
        "subtask" => cmd_subtask(&app, &args),
        "section" => cmd_section(&app, &args),
        "delete" => cmd_delete(&app, &args),
        "clean" => cmd_clean(&app, &args),
        "edit" => cmd_edit(&app, &args),
        "describe" | "note" => cmd_describe(&app, &args),
        "move" => cmd_move(&app, &args),
        "status" => cmd_status(&app, &args),
        "overdue" => cmd_overdue(&app, &args),
        "recur" => cmd_recur(&app, &args),
        "search" => cmd_search(&app, &args),
        "alarms" => cmd_alarms(&app, &args),
        "watch" => cmd_watch(&app, &args),
        "config" => cmd_config(&app, &args),
        "inbox" => cmd_show_single(&app, CoreFile::Inbox, &args),
        "today" => cmd_show_single(&app, CoreFile::Today, &args),
        "meta" => cmd_meta(&app, &args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => Err(format!("unknown command '{cmd}'\n\n{}", help_text())),
    }
}

struct AppPaths {
    root: PathBuf,
    current_file: PathBuf,
    config_file: PathBuf,
}

fn bootstrap_paths() -> io::Result<AppPaths> {
    let home = env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "$HOME is not set"))?;

    let root = home.join(".dodo");
    fs::create_dir_all(&root)?;

    for file in ["inbox.md", "today.md", "waiting.md", "someday.md"] {
        let path = root.join(file);
        if !path.exists() {
            fs::File::create(path)?;
        }
    }

    for dir in ["projects", "areas"] {
        fs::create_dir_all(root.join(dir))?;
    }

    let current = root.join(".current");
    if !current.exists() {
        write_string_atomic(&current, "inbox")?;
    }

    Ok(AppPaths {
        config_file: root.join("config.toml"),
        root,
        current_file: current,
    })
}

fn cmd_add(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: dodo add \"Task description\" [--file today|inbox|waiting|someday|projects/<name>] [--section NAME] [--due YYYY-MM-DD] [--tag TAG]".to_string());
    }

    let description = args[0].trim();
    if description.is_empty() {
        return Err("task description must not be empty".to_string());
    }
    let (parsed_description, nlp_meta) = parse_nlp_metadata(description);
    if parsed_description.is_empty() {
        return Err("task description must not be empty after parsing metadata".to_string());
    }

    let mut file = TaskFile::Core(CoreFile::Inbox);
    let mut due: Option<String> = None;
    let mut alarm: Option<String> = nlp_meta.time;
    let mut recurrence: Option<&'static str> = nlp_meta.recurrence;
    let mut priority: Option<&'static str> = nlp_meta.priority;
    let mut tags: Vec<String> = Vec::new();
    let mut section: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                i += 1;
                let name = args.get(i).ok_or("--file requires a value")?;
                file = parse_task_file(name).ok_or_else(|| {
                    "--file must be one of: inbox, today, waiting, someday, projects/<name>"
                        .to_string()
                })?;
            }
            "--due" => {
                i += 1;
                let date = args.get(i).ok_or("--due requires a value")?;
                if !is_valid_date(date) {
                    return Err("--due must be in YYYY-MM-DD format".to_string());
                }
                due = Some(date.clone());
            }
            "--section" => {
                i += 1;
                let name = args.get(i).ok_or("--section requires a value")?;
                let cleaned = name.trim();
                if cleaned.is_empty() {
                    return Err("--section must not be empty".to_string());
                }
                section = Some(cleaned.to_string());
            }
            "--tag" => {
                i += 1;
                let tag = args.get(i).ok_or("--tag requires a value")?;
                let cleaned = tag.trim().trim_start_matches('#');
                if cleaned.is_empty() {
                    return Err("--tag must not be empty".to_string());
                }
                tags.push(cleaned.to_string());
            }
            other => return Err(format!("unknown option '{other}'")),
        }
        i += 1;
    }

    let mut task = format!("- [ ] {}", parsed_description);
    for tag in tags {
        task.push(' ');
        task.push('#');
        task.push_str(&tag);
    }
    if due.is_none() {
        due = nlp_meta.due;
    }
    if let Some(date) = due {
        task.push_str(" 📅 ");
        task.push_str(&date);
    }
    if let Some(time) = alarm.take() {
        task.push_str(" ⏰ ");
        task.push_str(&time);
    }
    if let Some(rule) = recurrence.take() {
        task.push_str(" 🔁 ");
        task.push_str(rule);
    }
    if let Some(level) = priority.take() {
        task.push_str(" ⚡ ");
        task.push_str(level);
    }
    let task_id = generate_new_task_id(app)?;
    task.push_str(" 🆔 ");
    task.push_str(&task_id);

    let path = task_file_path(app, &file);
    if let Some(section_name) = section {
        insert_task_under_section(&path, &task, &section_name)
            .map_err(|e| format!("failed to write task: {e}"))?;
    } else {
        append_line(&path, &task).map_err(|e| format!("failed to write task: {e}"))?;
    }
    set_current_file(app, &file).map_err(|e| format!("failed to set current file: {e}"))?;

    println!(
        "{C_GREEN}Added{C_RESET} to {}{}{}:",
        C_CYAN,
        task_file_filename(&file),
        C_RESET
    );
    println!("  {C_BLUE}•{C_RESET} {}", task);
    Ok(())
}

struct NlpMetadata {
    due: Option<String>,
    time: Option<String>,
    recurrence: Option<&'static str>,
    priority: Option<&'static str>,
}

fn parse_nlp_metadata(description: &str) -> (String, NlpMetadata) {
    let tokens: Vec<&str> = description.split_whitespace().collect();
    let mut kept: Vec<String> = Vec::new();
    let mut due: Option<String> = None;
    let mut time: Option<String> = None;
    let mut recurrence: Option<&'static str> = None;
    let mut priority: Option<&'static str> = None;
    let today = today_ymd();

    let mut i = 0usize;
    while i < tokens.len() {
        let current = normalize_nlp_token(tokens[i]);

        if matches!(current.as_str(), "idag" | "today") {
            due = Some(today.clone());
            i += 1;
            continue;
        }
        if matches!(current.as_str(), "imorgon" | "tomorrow") {
            due = add_days_to_date(&today, 1);
            i += 1;
            continue;
        }

        if i + 2 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "om"
            && normalize_nlp_token(tokens[i + 2]) == "dagar"
        {
            if let Ok(days) = normalize_nlp_token(tokens[i + 1]).parse::<i64>() {
                due = add_days_to_date(&today, days.max(0));
                i += 3;
                continue;
            }
        }
        if i + 2 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "in"
            && normalize_nlp_token(tokens[i + 2]) == "days"
        {
            if let Ok(days) = normalize_nlp_token(tokens[i + 1]).parse::<i64>() {
                due = add_days_to_date(&today, days.max(0));
                i += 3;
                continue;
            }
        }

        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "nästa"
            && normalize_nlp_token(tokens[i + 1]) == "måndag"
        {
            due = next_weekday_date(&today, 1);
            i += 2;
            continue;
        }
        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "next"
            && normalize_nlp_token(tokens[i + 1]) == "monday"
        {
            due = next_weekday_date(&today, 1);
            i += 2;
            continue;
        }

        if i + 1 < tokens.len() && normalize_nlp_token(tokens[i]) == "kl" {
            if let Some(parsed) = parse_nlp_time_token(tokens[i + 1]) {
                time = Some(parsed);
                i += 2;
                continue;
            }
        }

        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "varje"
            && normalize_nlp_token(tokens[i + 1]) == "dag"
        {
            recurrence = Some("daily");
            i += 2;
            continue;
        }
        if normalize_nlp_token(tokens[i]) == "daily" {
            recurrence = Some("daily");
            i += 1;
            continue;
        }
        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "varje"
            && normalize_nlp_token(tokens[i + 1]) == "vecka"
        {
            recurrence = Some("weekly");
            i += 2;
            continue;
        }
        if normalize_nlp_token(tokens[i]) == "weekly" {
            recurrence = Some("weekly");
            i += 1;
            continue;
        }

        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "hög"
            && normalize_nlp_token(tokens[i + 1]) == "prio"
        {
            priority = Some("high");
            i += 2;
            continue;
        }
        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "high"
            && normalize_nlp_token(tokens[i + 1]) == "priority"
        {
            priority = Some("high");
            i += 2;
            continue;
        }
        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "låg"
            && normalize_nlp_token(tokens[i + 1]) == "prio"
        {
            priority = Some("low");
            i += 2;
            continue;
        }
        if i + 1 < tokens.len()
            && normalize_nlp_token(tokens[i]) == "low"
            && normalize_nlp_token(tokens[i + 1]) == "priority"
        {
            priority = Some("low");
            i += 2;
            continue;
        }

        kept.push(tokens[i].to_string());
        i += 1;
    }

    let cleaned = kept.join(" ");
    (
        cleaned.trim().to_string(),
        NlpMetadata {
            due,
            time,
            recurrence,
            priority,
        },
    )
}

fn normalize_nlp_token(token: &str) -> String {
    token
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .to_lowercase()
}

fn parse_nlp_time_token(value: &str) -> Option<String> {
    let normalized = normalize_nlp_token(value);
    if let Some((hh, mm)) = normalized.split_once(':') {
        if hh.len() == 2
            && mm.len() == 2
            && hh.chars().all(|c| c.is_ascii_digit())
            && mm.chars().all(|c| c.is_ascii_digit())
        {
            let hours = hh.parse::<u32>().ok()?;
            let mins = mm.parse::<u32>().ok()?;
            if hours < 24 && mins < 60 {
                return Some(format!("{hours:02}:{mins:02}"));
            }
        }
        return None;
    }

    if normalized.chars().all(|c| c.is_ascii_digit()) {
        let hours = normalized.parse::<u32>().ok()?;
        if hours < 24 {
            return Some(format!("{hours:02}:00"));
        }
    }
    None
}

fn cmd_list(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let mut selection = ListSelection::Inbox;
    let mut saw_file = false;
    let mut saw_tag = false;
    let mut json = false;
    let mut raw = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                json = true;
            }
            "--raw" => {
                raw = true;
            }
            "--file" => {
                saw_file = true;
                if saw_tag {
                    return Err("--file and --tag cannot be used together".to_string());
                }
                i += 1;
                let value = args.get(i).ok_or("--file requires a value")?;
                selection =
                    match value.as_str() {
                        "inbox" => ListSelection::Inbox,
                        "today" => ListSelection::Today,
                        "projects" => ListSelection::Projects,
                        "all" => ListSelection::All,
                        _ if is_project_arg(value) => {
                            ListSelection::Project(project_name_from_arg(value).to_string())
                        }
                        _ => return Err(
                            "--file must be one of: inbox, today, projects, projects/<name>, all"
                                .to_string(),
                        ),
                    };
            }
            "--tag" => {
                saw_tag = true;
                if saw_file {
                    return Err("--file and --tag cannot be used together".to_string());
                }
                i += 1;
                let value = args.get(i).ok_or("--tag requires a value")?;
                let cleaned = value.trim().trim_start_matches('#');
                if cleaned.is_empty() {
                    return Err("--tag must not be empty".to_string());
                }
                selection = ListSelection::Tag(cleaned.to_string());
            }
            other => return Err(format!("unknown option '{other}'")),
        }
        i += 1;
    }

    if json {
        let tasks = tasks_for_list_selection(app, &selection)?;
        print_tasks_json(&tasks);
        return Ok(());
    }

    match selection {
        ListSelection::Inbox => {
            print_tasks_for(app, CoreFile::Inbox, raw)?;
            set_current_file(app, &TaskFile::Core(CoreFile::Inbox)).map_err(|e| e.to_string())?;
        }
        ListSelection::Today => {
            print_tasks_for(app, CoreFile::Today, raw)?;
            set_current_file(app, &TaskFile::Core(CoreFile::Today)).map_err(|e| e.to_string())?;
        }
        ListSelection::Projects => {
            print_all_project_tasks(app, raw)?;
        }
        ListSelection::Project(name) => {
            let file = TaskFile::Project(name);
            print_tasks_for_task_file(app, &file, raw)?;
            set_current_file(app, &file).map_err(|e| e.to_string())?;
        }
        ListSelection::All => {
            print_tasks_all_global(app, raw)?;
        }
        ListSelection::Tag(tag) => {
            print_tasks_by_tag(app, &tag, raw)?;
        }
    }

    Ok(())
}

fn cmd_done(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let (file, idx, label_id) = if args.first().is_some_and(|arg| arg == "--id") {
        if args.len() != 2 && args.len() != 4 {
            return Err("usage: dodo done --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]".to_string());
        }
        let task_id = normalize_task_id(args.get(1).ok_or("--id requires a value")?)?;
        let selection = if args.len() == 4 {
            if args[2] != "--file" {
                return Err("usage: dodo done --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]".to_string());
            }
            parse_done_file(&args[3]).ok_or_else(|| {
                "--file must be one of: inbox, today, waiting, someday, projects/<name>, all"
                    .to_string()
            })?
        } else {
            DoneSelection::All
        };
        let (found_file, found_idx) = find_task_by_id(app, &task_id, Some(&selection))?
            .ok_or_else(|| format!("task with id {} not found", task_id))?;
        (found_file, found_idx, format!("id {task_id}"))
    } else {
        if args.len() != 3 {
            return Err(
                "usage: dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all"
                    .to_string(),
            );
        }

        let id: usize = args[0]
            .parse()
            .map_err(|_| "<id> must be a positive number".to_string())?;
        if id == 0 {
            return Err("<id> must be >= 1".to_string());
        }

        if args[1] != "--file" {
            return Err(
                "usage: dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all"
                    .to_string(),
            );
        }
        let selection = parse_done_file(&args[2]).ok_or_else(|| {
            "--file must be one of: inbox, today, waiting, someday, projects/<name>, all"
                .to_string()
        })?;

        match selection {
            DoneSelection::File(file) => {
                let path = task_file_path(app, &file);
                let lines = read_lines(&path)
                    .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
                let task_indices = task_line_indices(&lines);
                if id > task_indices.len() {
                    return Err(format!(
                        "task id {} not found in {} ({} tasks)",
                        id,
                        task_file_filename(&file),
                        task_indices.len()
                    ));
                }
                (file, task_indices[id - 1], format!("#{id}"))
            }
            DoneSelection::All => {
                let all_files =
                    all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))?;
                let mut global = 0usize;
                let mut found: Option<(TaskFile, usize)> = None;
                for file in all_files {
                    let path = task_file_path(app, &file);
                    let lines = read_lines(&path).map_err(|e| {
                        format!("failed to read {}: {e}", task_file_filename(&file))
                    })?;
                    for idx in task_line_indices(&lines) {
                        global += 1;
                        if global == id {
                            found = Some((file, idx));
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                let (file, idx) =
                    found.ok_or_else(|| format!("task id {} not found in --file all", id))?;
                (file, idx, format!("global #{id}"))
            }
        }
    };

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
    let original = lines[idx].clone();
    if original.starts_with("- [x]") {
        println!("{C_YELLOW}Already done{C_RESET}: {}", original);
        return Ok(());
    }

    let mut updated = original.replacen("- [ ]", "- [x]", 1);
    if !updated.contains('✅') {
        let today = today_ymd();
        updated.push_str(" ✅ ");
        updated.push_str(&today);
    }
    lines[idx] = updated.clone();

    write_lines(&path, &lines).map_err(|e| format!("failed to update task: {e}"))?;

    println!(
        "{C_GREEN}Done{C_RESET} in {}{}{} ({}):",
        C_CYAN,
        task_file_filename(&file),
        C_RESET,
        label_id
    );
    println!("  {C_BLUE}•{C_RESET} {}", updated);
    Ok(())
}

fn cmd_subtask(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let action = args.first().ok_or(
        "usage: dodo subtask add <parent-id> \"text\" | dodo subtask done <parent-id> <sub-num> | dodo subtask list <parent-id>",
    )?;
    match action.as_str() {
        "add" => {
            if args.len() < 3 {
                return Err("usage: dodo subtask add <parent-id> \"text\"".to_string());
            }
            let parent_id = normalize_task_id(&args[1])?;
            let text = args[2..].join(" ").trim().to_string();
            if text.is_empty() {
                return Err("subtask text must not be empty".to_string());
            }
            let (file, idx) = find_task_by_id(app, &parent_id, Some(&DoneSelection::All))?
                .ok_or_else(|| format!("task with id {} not found", parent_id))?;
            let path = task_file_path(app, &file);
            let mut lines = read_lines(&path)
                .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
            let insert_idx = task_block_end(&lines, idx);
            let subtask_line = format!("    - [ ] {}", text);
            lines.insert(insert_idx, subtask_line.clone());
            write_lines(&path, &lines).map_err(|e| format!("failed to update task: {e}"))?;
            println!(
                "{C_GREEN}Subtask added{C_RESET} in {}{}{} (id {}):",
                C_CYAN,
                task_file_filename(&file),
                C_RESET,
                parent_id
            );
            println!("  {C_BLUE}•{C_RESET} {}", subtask_line);
            Ok(())
        }
        "done" => {
            if args.len() != 3 {
                return Err("usage: dodo subtask done <parent-id> <sub-num>".to_string());
            }
            let parent_id = normalize_task_id(&args[1])?;
            let sub_num: usize = args[2]
                .parse()
                .map_err(|_| "<sub-num> must be a positive number".to_string())?;
            if sub_num == 0 {
                return Err("<sub-num> must be >= 1".to_string());
            }
            let (file, idx) = find_task_by_id(app, &parent_id, Some(&DoneSelection::All))?
                .ok_or_else(|| format!("task with id {} not found", parent_id))?;
            let path = task_file_path(app, &file);
            let mut lines = read_lines(&path)
                .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
            let subtasks = collect_subtask_indices(&lines, idx);
            if sub_num > subtasks.len() {
                return Err(format!(
                    "subtask {} not found for parent {} ({} subtasks)",
                    sub_num,
                    parent_id,
                    subtasks.len()
                ));
            }
            let sub_idx = subtasks[sub_num - 1];
            let original = lines[sub_idx].clone();
            if original.starts_with("    - [x] ") {
                println!("{C_YELLOW}Already done{C_RESET}: {}", original);
                return Ok(());
            }
            let mut updated = original.replacen("    - [ ]", "    - [x]", 1);
            if !updated.contains('✅') {
                updated.push_str(" ✅ ");
                updated.push_str(&today_ymd());
            }
            lines[sub_idx] = updated.clone();
            write_lines(&path, &lines).map_err(|e| format!("failed to update subtask: {e}"))?;
            println!(
                "{C_GREEN}Subtask done{C_RESET} in {}{}{} (parent {}, subtask {}):",
                C_CYAN,
                task_file_filename(&file),
                C_RESET,
                parent_id,
                sub_num
            );
            println!("  {C_BLUE}•{C_RESET} {}", updated);
            Ok(())
        }
        "list" => {
            if args.len() != 2 {
                return Err("usage: dodo subtask list <parent-id>".to_string());
            }
            let parent_id = normalize_task_id(&args[1])?;
            let (file, idx) = find_task_by_id(app, &parent_id, Some(&DoneSelection::All))?
                .ok_or_else(|| format!("task with id {} not found", parent_id))?;
            let path = task_file_path(app, &file);
            let lines = read_lines(&path)
                .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
            let subtasks = collect_subtasks(&lines, idx);
            println!(
                "{C_BOLD}{C_CYAN}Subtasks for {} ({}){C_RESET}",
                parent_id,
                task_file_filename(&file)
            );
            if subtasks.is_empty() {
                println!("  {C_BLUE}(none){C_RESET}");
                return Ok(());
            }
            for (i, subtask) in subtasks.iter().enumerate() {
                let marker = if subtask.done { "✓" } else { "□" };
                println!("  {:>2}. {} {}", i + 1, marker, subtask.text);
            }
            Ok(())
        }
        _ => Err(
            "usage: dodo subtask add <parent-id> \"text\" | dodo subtask done <parent-id> <sub-num> | dodo subtask list <parent-id>"
                .to_string(),
        ),
    }
}

fn cmd_section(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("add") {
        return Err("usage: dodo section add \"Name\" --file today|inbox|waiting|someday|projects/<name>".to_string());
    }
    if args.len() != 4 {
        return Err(
            "usage: dodo section add \"Name\" --file today|inbox|waiting|someday|projects/<name>"
                .to_string(),
        );
    }
    let name = args[1].trim();
    if name.is_empty() {
        return Err("section name must not be empty".to_string());
    }
    if args[2] != "--file" {
        return Err(
            "usage: dodo section add \"Name\" --file today|inbox|waiting|someday|projects/<name>"
                .to_string(),
        );
    }
    let file = parse_task_file(&args[3]).ok_or_else(|| {
        "--file must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
    })?;
    let path = task_file_path(app, &file);
    add_section_header(&path, name).map_err(|e| format!("failed to update file: {e}"))?;
    println!(
        "{C_GREEN}Section added{C_RESET} in {}{}{}:",
        C_CYAN,
        task_file_filename(&file),
        C_RESET
    );
    println!("  {C_BLUE}•{C_RESET} ## {}", name);
    Ok(())
}

fn cmd_delete(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let (file, remove_idx, label) = if args.first().is_some_and(|arg| arg == "--id") {
        if args.len() != 2 && args.len() != 4 {
            return Err("usage: dodo delete --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]".to_string());
        }
        let task_id = normalize_task_id(args.get(1).ok_or("--id requires a value")?)?;
        let selection = if args.len() == 4 {
            if args[2] != "--file" {
                return Err("usage: dodo delete --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]".to_string());
            }
            parse_done_file(&args[3]).ok_or_else(|| {
                "--file must be one of: inbox, today, waiting, someday, projects/<name>, all"
                    .to_string()
            })?
        } else {
            DoneSelection::All
        };
        let (found_file, found_idx) = find_task_by_id(app, &task_id, Some(&selection))?
            .ok_or_else(|| format!("task with id {} not found", task_id))?;
        (found_file, found_idx, format!("id {}", task_id))
    } else {
        if args.len() != 3 {
            return Err(
                "usage: dodo delete <id> --file inbox|today|waiting|someday|projects/<name>"
                    .to_string(),
            );
        }

        let id: usize = args[0]
            .parse()
            .map_err(|_| "<id> must be a positive number".to_string())?;
        if id == 0 {
            return Err("<id> must be >= 1".to_string());
        }

        if args[1] != "--file" {
            return Err(
                "usage: dodo delete <id> --file inbox|today|waiting|someday|projects/<name>"
                    .to_string(),
            );
        }

        let file = parse_task_file(&args[2]).ok_or_else(|| {
            "--file must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
        })?;

        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        let task_indices = task_line_indices(&lines);
        if id > task_indices.len() {
            return Err(format!(
                "task id {} not found in {} ({} tasks)",
                id,
                task_file_filename(&file),
                task_indices.len()
            ));
        }
        (file, task_indices[id - 1], format!("#{id}"))
    };

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
    let remove_end = task_block_end(&lines, remove_idx);
    let removed_line = lines[remove_idx].clone();
    lines.drain(remove_idx..remove_end);

    write_lines(&path, &lines)
        .map_err(|e| format!("failed to update {}: {e}", task_file_filename(&file)))?;
    println!(
        "Deleted from {} ({}): {}",
        task_file_filename(&file),
        label,
        removed_line
    );
    Ok(())
}

fn cmd_clean(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.len() != 2 || args[0] != "--file" {
        return Err(
            "usage: dodo clean --file inbox|today|waiting|someday|projects/<name>|all".to_string(),
        );
    }

    let selection = parse_done_file(&args[1]).ok_or_else(|| {
        "--file must be one of: inbox, today, waiting, someday, projects/<name>, all".to_string()
    })?;

    let files: Vec<TaskFile> = match selection {
        DoneSelection::File(file) => vec![file],
        DoneSelection::All => {
            all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))?
        }
    };

    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        let (cleaned, removed_count) = remove_completed_tasks(&lines);
        write_lines(&path, &cleaned)
            .map_err(|e| format!("failed to update {}: {e}", task_file_filename(&file)))?;
        println!(
            "Cleaned {}: removed {} completed tasks",
            task_file_filename(&file),
            removed_count
        );
    }

    Ok(())
}

fn cmd_edit(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let (file, idx, new_text, label) = if args.first().is_some_and(|arg| arg == "--id") {
        if args.len() < 3 {
            return Err("usage: dodo edit --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all] \"New task text\"".to_string());
        }
        let task_id = normalize_task_id(args.get(1).ok_or("--id requires a value")?)?;
        let mut text_start = 2usize;
        let mut selection = DoneSelection::All;
        if args.len() >= 5 && args[2] == "--file" {
            selection = parse_done_file(&args[3]).ok_or_else(|| {
                "--file must be one of: inbox, today, waiting, someday, projects/<name>, all"
                    .to_string()
            })?;
            text_start = 4;
        }
        let new_text = args[text_start..].join(" ").trim().to_string();
        let (file, idx) = find_task_by_id(app, &task_id, Some(&selection))?
            .ok_or_else(|| format!("task with id {} not found", task_id))?;
        (file, idx, new_text, format!("id {}", task_id))
    } else {
        if args.len() < 4 {
            return Err(
                "usage: dodo edit <id> --file inbox|today|waiting|someday|projects/<name> \"New task text\""
                    .to_string(),
            );
        }

        let id: usize = args[0]
            .parse()
            .map_err(|_| "<id> must be a positive number".to_string())?;
        if id == 0 {
            return Err("<id> must be >= 1".to_string());
        }
        if args[1] != "--file" {
            return Err(
                "usage: dodo edit <id> --file inbox|today|waiting|someday|projects/<name> \"New task text\""
                    .to_string(),
            );
        }

        let file = parse_task_file(&args[2]).ok_or_else(|| {
            "--file must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
        })?;
        let new_text = args[3..].join(" ").trim().to_string();

        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        let task_indices = task_line_indices(&lines);
        if id > task_indices.len() {
            return Err(format!(
                "task id {} not found in {} ({} tasks)",
                id,
                task_file_filename(&file),
                task_indices.len()
            ));
        }

        (file, task_indices[id - 1], new_text, format!("#{id}"))
    };

    if new_text.is_empty() {
        return Err("new task text must not be empty".to_string());
    }

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
    let original = &lines[idx];
    let prefix = if original.starts_with("- [x] ") {
        "- [x] "
    } else {
        "- [ ] "
    };
    let mut updated = format!("{prefix}{new_text}");
    if let Some(existing_id) = extract_task_id(original) {
        if extract_task_id(&updated).is_none() {
            updated.push_str(" 🆔 ");
            updated.push_str(&existing_id);
        }
    }
    lines[idx] = updated.clone();
    write_lines(&path, &lines).map_err(|e| format!("failed to update task: {e}"))?;

    println!(
        "{C_GREEN}Edited{C_RESET} in {}{}{} ({}):",
        C_CYAN,
        task_file_filename(&file),
        C_RESET,
        label
    );
    println!("  {C_BLUE}•{C_RESET} {}", updated);
    Ok(())
}

fn cmd_move(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.len() != 2 && args.len() != 3 {
        return Err("usage: dodo move <id> <file> | dodo move --id <6-hex> <file>".to_string());
    }

    if args[0] == "--id" {
        if args.len() != 3 {
            return Err("usage: dodo move --id <6-hex> <file>".to_string());
        }
        let task_id = normalize_task_id(&args[1])?;
        let target = parse_task_file(&args[2]).ok_or_else(|| {
            "<file> must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
        })?;
        let (source_file, remove_idx) = find_task_by_id(app, &task_id, Some(&DoneSelection::All))?
            .ok_or_else(|| format!("task with id {} not found", task_id))?;
        move_task_block(
            app,
            &source_file,
            remove_idx,
            &target,
            format!("id {}", task_id),
        )
    } else {
        let id: usize = args[0]
            .parse()
            .map_err(|_| "<id> must be a positive number".to_string())?;
        if id == 0 {
            return Err("<id> must be >= 1".to_string());
        }
        let target = parse_task_file(&args[1]).ok_or_else(|| {
            "<file> must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
        })?;

        let inbox_path = app.root.join(CoreFile::Inbox.filename());
        let inbox_lines =
            read_lines(&inbox_path).map_err(|e| format!("failed to read inbox: {e}"))?;
        let inbox_task_indices = task_line_indices(&inbox_lines);
        if id > inbox_task_indices.len() {
            return Err(format!(
                "task id {} not found in inbox ({})",
                id,
                inbox_task_indices.len()
            ));
        }
        move_task_block(
            app,
            &TaskFile::Core(CoreFile::Inbox),
            inbox_task_indices[id - 1],
            &target,
            format!("#{id}"),
        )
    }
}

fn cmd_describe(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.len() < 4 {
        return Err(
            "usage: dodo describe <id> --file inbox|today|waiting|someday|projects/<name> \"Description text\""
                .to_string(),
        );
    }

    let id: usize = args[0]
        .parse()
        .map_err(|_| "<id> must be a positive number".to_string())?;
    if id == 0 {
        return Err("<id> must be >= 1".to_string());
    }
    if args[1] != "--file" {
        return Err(
            "usage: dodo describe <id> --file inbox|today|waiting|someday|projects/<name> \"Description text\""
                .to_string(),
        );
    }

    let file = parse_task_file(&args[2]).ok_or_else(|| {
        "--file must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
    })?;
    let note = args[3..].join(" ").trim().to_string();
    if note.is_empty() {
        return Err("description text must not be empty".to_string());
    }

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
    let task_indices = task_line_indices(&lines);
    if id > task_indices.len() {
        return Err(format!(
            "task id {} not found in {} ({} tasks)",
            id,
            task_file_filename(&file),
            task_indices.len()
        ));
    }

    let idx = task_indices[id - 1];
    let insert_idx = task_block_end(&lines, idx);
    let desc_line = format!("  {}", note);
    lines.insert(insert_idx, desc_line.clone());

    write_lines(&path, &lines).map_err(|e| format!("failed to update task: {e}"))?;

    println!(
        "{C_GREEN}Described{C_RESET} in {}{}{} (#{id}):",
        C_CYAN,
        task_file_filename(&file),
        C_RESET
    );
    println!("  {C_BLUE}•{C_RESET} {}", desc_line);
    Ok(())
}

fn cmd_show_single(app: &AppPaths, file: CoreFile, args: &[String]) -> Result<(), String> {
    let mut json = false;
    for arg in args {
        if arg == "--json" {
            json = true;
        } else {
            return Err(format!("unknown option '{arg}'"));
        }
    }
    if json {
        let file_ref = TaskFile::Core(file);
        let tasks = collect_tasks_for_file(app, &file_ref)?;
        print_tasks_json(&tasks);
        set_current_file(app, &file_ref).map_err(|e| e.to_string())?;
        return Ok(());
    }

    print_tasks_for(app, file, false)?;
    set_current_file(app, &TaskFile::Core(file)).map_err(|e| e.to_string())?;
    Ok(())
}

fn cmd_meta(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let mut file: Option<TaskFile> = None;
    let mut set_value: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                i += 1;
                let value = args.get(i).ok_or("--file requires a value")?;
                file = Some(parse_task_file(value).ok_or_else(|| {
                    "--file must be one of: inbox, today, waiting, someday, projects/<name>"
                        .to_string()
                })?);
            }
            "--set" => {
                i += 1;
                set_value = Some(args.get(i).ok_or("--set requires a value")?.clone());
            }
            other => return Err(format!("unknown option '{other}'")),
        }
        i += 1;
    }

    let file = file.ok_or("usage: dodo meta --file inbox|today|waiting|someday|projects/<name> [--set \"key: value\"]")?;
    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
    let (mut frontmatter, body_start) = parse_frontmatter(&lines);

    if let Some(kv) = set_value {
        let (key, value) = parse_meta_set_value(&kv)?;
        let mut updated = false;
        for (k, v) in &mut frontmatter {
            if *k == key {
                *v = value.clone();
                updated = true;
                break;
            }
        }
        if !updated {
            frontmatter.push((key, value));
        }
        let body: Vec<String> = lines.drain(body_start..).collect();
        let updated_lines = with_frontmatter(&frontmatter, &body);
        write_lines(&path, &updated_lines)
            .map_err(|e| format!("failed to update {}: {e}", task_file_filename(&file)))?;
        for (k, v) in frontmatter {
            println!("{k}: {v}");
        }
        return Ok(());
    }

    for (k, v) in frontmatter {
        println!("{k}: {v}");
    }
    Ok(())
}

fn cmd_status(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let mut json = false;
    for arg in args {
        if arg == "--json" {
            json = true;
        } else {
            return Err("usage: dodo status [--json]".to_string());
        }
    }

    let mut files: Vec<TaskFile> = vec![
        TaskFile::Core(CoreFile::Inbox),
        TaskFile::Core(CoreFile::Today),
        TaskFile::Core(CoreFile::Waiting),
        TaskFile::Core(CoreFile::Someday),
    ];
    let projects = project_files(app).map_err(|e| format!("failed to read projects/: {e}"))?;
    let project_count = projects.len();
    files.extend(projects);

    let today = today_ymd();
    let mut total_open = 0usize;
    let mut overdue_tasks: Vec<OverdueTask> = Vec::new();

    if !json {
        println!("{C_BOLD}{C_CYAN}Status{C_RESET}");
        println!("{C_BOLD}{C_CYAN}Open Tasks By File{C_RESET}");
    }
    let mut open_by_file: Vec<(String, usize)> = Vec::new();
    for file in &files {
        let path = task_file_path(app, file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
        let open_count = task_line_indices(&lines)
            .into_iter()
            .filter(|idx| is_open_task_line(&lines[*idx]))
            .count();
        total_open += open_count;
        open_by_file.push((task_file_filename(file), open_count));
        if !json {
            println!(
                "  {C_BLUE}•{C_RESET} {}{}{}: {}",
                C_CYAN,
                task_file_filename(file),
                C_RESET,
                open_count
            );
        }

        for idx in task_line_indices(&lines) {
            let line = &lines[idx];
            if !is_open_task_line(line) {
                continue;
            }
            if let Some(due) = extract_due_date(line) {
                if due.as_str() < today.as_str() {
                    overdue_tasks.push(OverdueTask {
                        due,
                        file: task_file_filename(file),
                        line: line.clone(),
                        descriptions: collect_descriptions(&lines, idx),
                    });
                }
            }
        }
    }

    overdue_tasks.sort_by(|a, b| a.due.cmp(&b.due));
    if !json {
        println!();
        println!("{C_BOLD}{C_CYAN}Overdue Tasks{C_RESET}");
        if overdue_tasks.is_empty() {
            println!("  {C_GREEN}(none){C_RESET}");
        } else {
            for task in &overdue_tasks {
                println!(
                    "  {C_RED}• [{}] {} ({}){C_RESET}",
                    task.file, task.line, task.due
                );
                for description in &task.descriptions {
                    println!(
                        "      {C_DIM}{}{C_RESET}",
                        description.trim_start_matches("  ")
                    );
                }
            }
        }
    }

    let inbox_path = app.root.join(CoreFile::Inbox.filename());
    let inbox_lines =
        read_lines(&inbox_path).map_err(|e| format!("failed to read inbox.md: {e}"))?;
    let stale_inbox_count = stale_inbox_count(&inbox_path, &inbox_lines)?;
    if json {
        print_status_json(
            total_open,
            overdue_tasks.len(),
            project_count,
            stale_inbox_count,
            &open_by_file,
        );
    } else {
        println!();
        println!("{C_BOLD}{C_CYAN}Stale Inbox{C_RESET}");
        println!(
            "  {C_BLUE}•{C_RESET} {} undated open task(s) older than 3 days (file mtime proxy)",
            stale_inbox_count
        );

        println!();
        println!(
            "{C_BOLD}{} open tasks, {} overdue, {} projects{C_RESET}",
            total_open,
            overdue_tasks.len(),
            project_count
        );
    }

    Ok(())
}

fn cmd_overdue(app: &AppPaths, args: &[String]) -> Result<(), String> {
    let mut json = false;
    for arg in args {
        if arg == "--json" {
            json = true;
        } else {
            return Err("usage: dodo overdue [--json]".to_string());
        }
    }

    let mut files: Vec<TaskFile> = vec![
        TaskFile::Core(CoreFile::Inbox),
        TaskFile::Core(CoreFile::Today),
        TaskFile::Core(CoreFile::Waiting),
        TaskFile::Core(CoreFile::Someday),
    ];
    files.extend(project_files(app).map_err(|e| format!("failed to read projects/: {e}"))?);

    let today = today_ymd();
    let mut overdue_tasks: Vec<OverdueTask> = Vec::new();
    for file in &files {
        let path = task_file_path(app, file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
        for idx in task_line_indices(&lines) {
            let line = &lines[idx];
            if !is_open_task_line(line) {
                continue;
            }
            if let Some(due) = extract_due_date(line) {
                if due.as_str() < today.as_str() {
                    overdue_tasks.push(OverdueTask {
                        due,
                        file: task_file_filename(file),
                        line: line.clone(),
                        descriptions: collect_descriptions(&lines, idx),
                    });
                }
            }
        }
    }

    overdue_tasks.sort_by(|a, b| a.due.cmp(&b.due));

    if json {
        let tasks: Vec<TaskJsonRecord> = overdue_tasks
            .into_iter()
            .map(|task| {
                let parsed = parse_task_line(&task.line);
                TaskJsonRecord {
                    id: parsed.as_ref().and_then(|p| p.id.clone()),
                    done: parsed.as_ref().is_some_and(|p| p.done),
                    text: parsed
                        .as_ref()
                        .map_or_else(|| task.line.clone(), |p| p.text.clone()),
                    tags: parsed.map_or_else(Vec::new, |p| p.tags),
                    due: Some(task.due),
                    file: task.file,
                    description: task
                        .descriptions
                        .into_iter()
                        .map(|d| d.trim_start_matches("  ").to_string())
                        .collect(),
                    subtasks: Vec::new(),
                }
            })
            .collect();
        print_tasks_json(&tasks);
    } else {
        println!("{C_BOLD}{C_CYAN}Overdue Tasks{C_RESET}");
        if overdue_tasks.is_empty() {
            println!("  {C_GREEN}(none){C_RESET}");
        } else {
            for task in overdue_tasks {
                println!(
                    "  {C_RED}• [{}] {} ({}){C_RESET}",
                    task.file, task.line, task.due
                );
                for description in &task.descriptions {
                    println!(
                        "      {C_DIM}{}{C_RESET}",
                        description.trim_start_matches("  ")
                    );
                }
            }
        }
    }

    Ok(())
}

fn cmd_recur(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: dodo recur".to_string());
    }
    run_recur(app, true).map(|_| ())
}

fn cmd_search(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: dodo search [--json] \"query\"".to_string());
    }
    let mut json = false;
    let mut parts: Vec<&str> = Vec::new();
    for arg in args {
        if arg == "--json" {
            json = true;
        } else {
            parts.push(arg);
        }
    }
    if parts.is_empty() {
        return Err("usage: dodo search [--json] \"query\"".to_string());
    }
    let query = parts.join(" ");
    let needle = query.to_lowercase();

    let mut json_results: Vec<TaskJsonRecord> = Vec::new();
    let files = all_task_files(app).map_err(|e| format!("failed to read files: {e}"))?;
    let mut printed_any = false;
    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        let mut matched: Vec<(String, Vec<String>)> = Vec::new();
        for idx in task_line_indices(&lines) {
            let line = &lines[idx];
            let descriptions = collect_descriptions(&lines, idx);
            let parsed = parse_task_line(line);
            let task_text = parsed
                .as_ref()
                .map_or_else(|| line.clone(), |p| p.text.clone());
            let task_hit = contains_case_insensitive(&task_text, &needle);
            let desc_hit = descriptions
                .iter()
                .any(|d| contains_case_insensitive(d.trim_start_matches("  "), &needle));
            if !task_hit && !desc_hit {
                continue;
            }

            if json {
                if let Some(parsed) = parsed {
                    json_results.push(TaskJsonRecord {
                        id: parsed.id,
                        done: parsed.done,
                        text: parsed.text,
                        tags: parsed.tags,
                        due: parsed.due,
                        file: task_file_filename(&file),
                        description: descriptions
                            .iter()
                            .map(|d| d.trim_start_matches("  ").to_string())
                            .collect(),
                        subtasks: collect_subtasks(&lines, idx)
                            .into_iter()
                            .map(|s| SubtaskJsonRecord {
                                done: s.done,
                                text: s.text,
                                done_at: s.done_at,
                            })
                            .collect(),
                    });
                } else {
                    json_results.push(TaskJsonRecord {
                        id: None,
                        done: line.starts_with("- [x] "),
                        text: line.clone(),
                        tags: Vec::new(),
                        due: extract_due_date(line),
                        file: task_file_filename(&file),
                        description: descriptions
                            .iter()
                            .map(|d| d.trim_start_matches("  ").to_string())
                            .collect(),
                        subtasks: collect_subtasks(&lines, idx)
                            .into_iter()
                            .map(|s| SubtaskJsonRecord {
                                done: s.done,
                                text: s.text,
                                done_at: s.done_at,
                            })
                            .collect(),
                    });
                }
            } else {
                matched.push((line.clone(), descriptions));
            }
        }

        if json || matched.is_empty() {
            continue;
        }
        if printed_any {
            println!();
        }
        printed_any = true;
        println!(
            "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
            task_file_label(&file),
            task_file_filename(&file)
        );
        for (i, (line, descriptions)) in matched.iter().enumerate() {
            print_task_row(i + 1, line, false, None);
            for description in descriptions {
                println!(
                    "      {C_DIM}{}{C_RESET}",
                    description.trim_start_matches("  ")
                );
            }
        }
    }

    if json {
        print_tasks_json(&json_results);
    } else if !printed_any {
        println!("  {C_BLUE}(no matches){C_RESET}");
    }
    Ok(())
}

fn cmd_alarms(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: dodo alarms".to_string());
    }
    let now = now_ymd_hm();
    let alarms: Vec<AlarmRecord> = collect_all_alarms(app)?
        .into_iter()
        .filter(|a| a.when >= now)
        .collect();
    println!("{C_BOLD}{C_CYAN}Alarms{C_RESET}");
    if alarms.is_empty() {
        println!("  {C_BLUE}(none){C_RESET}");
        return Ok(());
    }
    for (i, alarm) in alarms.iter().enumerate() {
        let id = i + 1;
        let marker = if alarm.line.starts_with("- [x] ") {
            "✓"
        } else {
            "□"
        };
        let parsed = parse_display_task_line(&alarm.line);
        let text = parsed
            .as_ref()
            .map_or_else(|| alarm.line.clone(), |p| p.text.clone());
        println!(
            "  {:>2}. {} {}  {}",
            id,
            marker,
            text,
            alarm.when
        );
        println!(
            "      {C_DIM}{}{C_RESET}",
            task_file_filename(&alarm.file)
        );
    }
    Ok(())
}

fn cmd_watch(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: dodo watch".to_string());
    }
    let created = run_recur(app, true)?;
    if created > 0 {
        println!("Processed recurring tasks: {created}");
    }
    let config = read_app_config(app)?;
    loop {
        let now = now_ymd_hm();
        let pending = collect_all_alarms(app)?;
        if let Some(alarm) = pending.into_iter().find(|item| item.when <= now) {
            trigger_alarm_handler(&config.watch.handler, &alarm)?;
            return Ok(());
        }
        thread::sleep(Duration::from_secs(config.watch.interval.max(1)));
    }
}

fn cmd_config(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: dodo config get <key> | dodo config set <key> <value>".to_string());
    }
    match args[0].as_str() {
        "get" => {
            if args.len() != 2 {
                return Err("usage: dodo config get watch.handler|watch.interval".to_string());
            }
            let cfg = read_app_config(app)?;
            match args[1].as_str() {
                "watch.handler" => println!("{}", cfg.watch.handler),
                "watch.interval" => println!("{}", cfg.watch.interval),
                _ => {
                    return Err(
                        "config key must be watch.handler or watch.interval".to_string()
                    )
                }
            }
            Ok(())
        }
        "set" => {
            if args.len() != 3 {
                return Err(
                    "usage: dodo config set watch.handler|watch.interval <value>".to_string(),
                );
            }
            let value = match args[1].as_str() {
                "watch.handler" => ConfigValue::Text(args[2].clone()),
                "watch.interval" => {
                    let interval = args[2]
                        .parse::<u64>()
                        .map_err(|_| "watch.interval must be a positive integer".to_string())?;
                    if interval == 0 {
                        return Err("watch.interval must be >= 1".to_string());
                    }
                    ConfigValue::Number(interval)
                }
                _ => return Err("config key must be watch.handler or watch.interval".to_string()),
            };
            set_config_value(app, &args[1], value)?;
            println!("{} = {}", args[1], args[2]);
            Ok(())
        }
        _ => Err("usage: dodo config get <key> | dodo config set <key> <value>".to_string()),
    }
}

fn print_tasks_for(app: &AppPaths, file: CoreFile, raw: bool) -> Result<(), String> {
    let path = app.root.join(file.filename());
    let lines =
        read_lines(&path).map_err(|e| format!("failed to read {}: {e}", file.filename()))?;

    println!(
        "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
        file.label(),
        file.filename()
    );

    let shown = print_numbered_tasks(&lines, 0, raw);

    if shown == 0 {
        println!("  {C_BLUE}(no tasks){C_RESET}");
    }

    Ok(())
}

fn print_tasks_for_task_file(app: &AppPaths, file: &TaskFile, raw: bool) -> Result<(), String> {
    let path = task_file_path(app, file);
    let lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;

    println!(
        "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
        task_file_label(file),
        task_file_filename(file)
    );

    let shown = print_numbered_tasks(&lines, 0, raw);

    if shown == 0 {
        println!("  {C_BLUE}(no tasks){C_RESET}");
    }

    Ok(())
}

fn print_all_project_tasks(app: &AppPaths, raw: bool) -> Result<(), String> {
    let projects = project_files(app).map_err(|e| format!("failed to read projects/: {e}"))?;
    if projects.is_empty() {
        println!("{C_BOLD}{C_CYAN}Projects (projects/){C_RESET}");
        println!("  {C_BLUE}(no tasks){C_RESET}");
        return Ok(());
    }

    for (idx, project) in projects.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        print_tasks_for_task_file(app, project, raw)?;
    }
    Ok(())
}

fn print_tasks_all_global(app: &AppPaths, raw: bool) -> Result<(), String> {
    let files = all_task_files(app).map_err(|e| format!("failed to read files: {e}"))?;
    let mut shown = 0usize;
    for (file_idx, file) in files.iter().enumerate() {
        if file_idx > 0 {
            println!();
        }
        let path = task_file_path(app, file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
        println!(
            "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
            task_file_label(file),
            task_file_filename(file)
        );
        let previous = shown;
        shown = print_numbered_tasks(&lines, shown, raw);
        let any = shown > previous;
        if !any {
            println!("  {C_BLUE}(no tasks){C_RESET}");
        }
    }
    Ok(())
}

fn print_tasks_by_tag(app: &AppPaths, tag: &str, raw: bool) -> Result<(), String> {
    let files = all_task_files(app).map_err(|e| format!("failed to read files: {e}"))?;
    let needle = format!("#{}", tag.trim_start_matches('#'));
    let mut found_any = false;

    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        let mut matched: Vec<(String, Vec<String>)> = Vec::new();
        for idx in task_line_indices(&lines) {
            let line = &lines[idx];
            if line.contains(&needle) {
                matched.push((line.clone(), collect_descriptions(&lines, idx)));
            }
        }
        if matched.is_empty() {
            continue;
        }

        if found_any {
            println!();
        }
        found_any = true;
        println!(
            "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
            task_file_label(&file),
            task_file_filename(&file)
        );
        for (i, (line, descriptions)) in matched.iter().enumerate() {
            let id = i + 1;
            print_task_row(id, line, raw, None);
            for description in descriptions {
                println!(
                    "      {C_DIM}{}{C_RESET}",
                    description.trim_start_matches("  ")
                );
            }
        }
    }

    if !found_any {
        println!("{C_BOLD}{C_CYAN}Tag: {needle}{C_RESET}");
        println!("  {C_BLUE}(no tasks){C_RESET}");
    }

    Ok(())
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(f, "{}", line)
}

fn append_lines(path: &Path, lines: &[String]) -> io::Result<()> {
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    for line in lines {
        writeln!(f, "{}", line)?;
    }
    Ok(())
}

fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    Ok(content.lines().map(|s| s.to_string()).collect())
}

fn write_lines(path: &Path, lines: &[String]) -> io::Result<()> {
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(path, out)
}

fn write_string_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)
}

fn section_title(line: &str) -> Option<&str> {
    line.strip_prefix("## ").map(str::trim).filter(|s| !s.is_empty())
}

fn add_section_header(path: &Path, name: &str) -> io::Result<()> {
    let mut lines = if path.exists() {
        read_lines(path)?
    } else {
        Vec::new()
    };
    let target = name.trim();
    if lines
        .iter()
        .any(|line| section_title(line).is_some_and(|s| s == target))
    {
        return Ok(());
    }
    if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
        lines.push(String::new());
    }
    lines.push(format!("## {}", target));
    write_lines(path, &lines)
}

fn insert_task_under_section(path: &Path, task: &str, section: &str) -> io::Result<()> {
    let mut lines = if path.exists() {
        read_lines(path)?
    } else {
        Vec::new()
    };
    let body_start = frontmatter_body_start(&lines);
    let target = section.trim();
    let mut section_idx: Option<usize> = None;
    for idx in body_start..lines.len() {
        if section_title(&lines[idx]).is_some_and(|name| name == target) {
            section_idx = Some(idx);
            break;
        }
    }
    if let Some(header_idx) = section_idx {
        let mut insert_idx = header_idx + 1;
        while insert_idx < lines.len() && !is_section_header(&lines[insert_idx]) {
            insert_idx += 1;
        }
        lines.insert(insert_idx, task.to_string());
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("## {}", target));
        lines.push(task.to_string());
    }
    write_lines(path, &lines)
}

fn contains_case_insensitive(haystack: &str, needle_lower: &str) -> bool {
    haystack.to_lowercase().contains(needle_lower)
}

fn run_recur(_app: &AppPaths, _quiet: bool) -> Result<usize, String> {
    Ok(0)
}

fn task_line_indices(lines: &[String]) -> Vec<usize> {
    let body_start = frontmatter_body_start(lines);
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            if i >= body_start && is_top_level_task_line(line) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn is_task_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] ")
}

fn is_top_level_task_line(line: &str) -> bool {
    line.starts_with("- [ ] ") || line.starts_with("- [x] ")
}

fn is_subtask_line(line: &str) -> bool {
    line.starts_with("    - [ ] ") || line.starts_with("    - [x] ")
}

fn is_section_header(line: &str) -> bool {
    line.starts_with("## ") && line.trim().len() > 3
}

fn is_description_line(line: &str) -> bool {
    line.strip_prefix("  ")
        .is_some_and(|rest| !rest.starts_with(' '))
        && !is_task_line(line)
}

fn task_block_end(lines: &[String], start_idx: usize) -> usize {
    let mut idx = start_idx + 1;
    while idx < lines.len() {
        if is_top_level_task_line(&lines[idx]) || is_section_header(&lines[idx]) {
            break;
        }
        idx += 1;
    }
    idx
}

fn collect_descriptions(lines: &[String], task_idx: usize) -> Vec<String> {
    let mut descriptions = Vec::new();
    let mut idx = task_idx + 1;
    let block_end = task_block_end(lines, task_idx);
    while idx < block_end {
        if is_description_line(&lines[idx]) {
            descriptions.push(lines[idx].clone());
        }
        idx += 1;
    }
    descriptions
}

fn parse_subtask_line(line: &str) -> Option<SubtaskLine> {
    let (done, body) = if let Some(rest) = line.strip_prefix("    - [x] ") {
        (true, rest)
    } else if let Some(rest) = line.strip_prefix("    - [ ] ") {
        (false, rest)
    } else {
        return None;
    };
    let mut text = body.to_string();
    let mut done_at = None;
    if let Some((head, tail)) = text.rsplit_once(" ✅ ") {
        let token = tail.split_whitespace().next().unwrap_or("");
        if is_valid_date(token) {
            done_at = Some(token.to_string());
            text = head.to_string();
        }
    }
    Some(SubtaskLine {
        done,
        text: text.trim().to_string(),
        done_at,
    })
}

fn collect_subtask_indices(lines: &[String], task_idx: usize) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut idx = task_idx + 1;
    let block_end = task_block_end(lines, task_idx);
    while idx < block_end {
        if is_subtask_line(&lines[idx]) {
            indices.push(idx);
        }
        idx += 1;
    }
    indices
}

fn collect_subtasks(lines: &[String], task_idx: usize) -> Vec<SubtaskLine> {
    collect_subtask_indices(lines, task_idx)
        .into_iter()
        .filter_map(|idx| parse_subtask_line(&lines[idx]))
        .collect()
}

fn subtask_progress_label(lines: &[String], task_idx: usize) -> Option<String> {
    let subtasks = collect_subtasks(lines, task_idx);
    if subtasks.is_empty() {
        return None;
    }
    let done = subtasks.iter().filter(|s| s.done).count();
    Some(format!("({}/{})", done, subtasks.len()))
}

fn remove_completed_tasks(lines: &[String]) -> (Vec<String>, usize) {
    let mut kept: Vec<String> = Vec::with_capacity(lines.len());
    let mut removed_count = 0usize;
    let body_start = frontmatter_body_start(lines);
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = &lines[idx];
        if idx >= body_start && line.starts_with("- [x] ") {
            removed_count += 1;
            idx = task_block_end(lines, idx);
            continue;
        }
        kept.push(line.clone());
        idx += 1;
    }
    (kept, removed_count)
}

fn print_numbered_tasks(lines: &[String], start_id: usize, raw: bool) -> usize {
    let mut shown = start_id;
    let body_start = frontmatter_body_start(lines);
    let mut printed_section = false;
    for (idx, line) in lines.iter().enumerate() {
        if idx < body_start {
            continue;
        }
        if is_section_header(line) {
            if printed_section {
                println!();
            }
            printed_section = true;
            println!("  {C_BOLD}{}{C_RESET}", line);
            continue;
        }
        if !is_top_level_task_line(line) {
            continue;
        }

        shown += 1;
        let progress = subtask_progress_label(lines, idx);
        print_task_row(shown, line, raw, progress.as_deref());

        for description in collect_descriptions(lines, idx) {
            println!(
                "      {C_DIM}{}{C_RESET}",
                description.trim_start_matches("  ")
            );
        }
        for subtask in collect_subtasks(lines, idx) {
            let check = if subtask.done { "[x]" } else { "[ ]" };
            let mut sub_line = format!("- {} {}", check, subtask.text);
            if let Some(done_at) = subtask.done_at {
                sub_line.push_str(" ✅ ");
                sub_line.push_str(&done_at);
            }
            println!("      {}", sub_line);
        }
    }
    shown
}

fn print_task_row(index: usize, line: &str, raw: bool, progress: Option<&str>) {
    if raw {
        let suffix = progress.map_or_else(String::new, |p| format!(" {}", p));
        if line.starts_with("- [x]") {
            println!("  {C_GREEN}{:>2}.{C_RESET} {}{}", index, line, suffix);
        } else {
            println!("  {C_YELLOW}{:>2}.{C_RESET} {}{}", index, line, suffix);
        }
        return;
    }

    let Some(parsed) = parse_display_task_line(line) else {
        let suffix = progress.map_or_else(String::new, |p| format!(" {}", p));
        if line.starts_with("- [x]") {
            println!("  {C_GREEN}{:>2}.{C_RESET} {}{}", index, line, suffix);
        } else {
            println!("  {C_YELLOW}{:>2}.{C_RESET} {}{}", index, line, suffix);
        }
        return;
    };

    let marker = if parsed.done { "✓" } else { "□" };
    let mut left_text = parsed.text.clone();
    if let Some(label) = progress {
        left_text.push(' ');
        left_text.push_str(C_DIM);
        left_text.push_str(label);
        left_text.push_str(C_RESET);
    }
    if parsed.done {
        left_text.push_str(" (done)");
    }
    if !parsed.tags.is_empty() {
        left_text.push(' ');
        left_text.push_str(C_DIM);
        for (i, tag) in parsed.tags.iter().enumerate() {
            if i > 0 {
                left_text.push(' ');
            }
            left_text.push('#');
            left_text.push_str(tag);
        }
        left_text.push_str(C_RESET);
    }

    let right = task_right_side(&parsed);
    let left = format!("  {:>2}. {} {}", index, marker, left_text);
    if right.is_empty() {
        println!("{left}");
    } else {
        let target_col = 48usize;
        let left_len = printable_len(&left);
        let pad = if left_len >= target_col {
            1
        } else {
            target_col - left_len
        };
        println!("{left}{}{right}", " ".repeat(pad));
    }
}

fn task_right_side(task: &DisplayTaskLine) -> String {
    let mut parts: Vec<String> = Vec::new();
    if task.done {
        if let Some(done_at) = &task.done_at {
            parts.push(human_date_label(done_at));
        } else if let Some(date) = task
            .due
            .as_deref()
            .or_else(|| alarm_date_part(task.alarm.as_ref()))
        {
            parts.push(human_date_label(date));
        }
    } else if let Some(date) = task
        .due
        .as_deref()
        .or_else(|| alarm_date_part(task.alarm.as_ref()))
    {
        parts.push(human_date_label(date));
    }

    if let Some(time) = alarm_time_part(task.alarm.as_ref()) {
        parts.push(time.to_string());
    }

    parts.join(" ")
}

fn printable_len(value: &str) -> usize {
    let mut out = 0usize;
    let mut in_escape = false;
    for ch in value.chars() {
        if in_escape {
            if ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        out += 1;
    }
    out
}

fn is_open_task_line(line: &str) -> bool {
    line.starts_with("- [ ] ")
}

fn extract_due_date(line: &str) -> Option<String> {
    let (_, tail) = line.split_once("📅 ")?;
    let date = tail.split_whitespace().next()?;
    if is_valid_date(date) {
        Some(date.to_string())
    } else {
        None
    }
}

fn is_valid_date(value: &str) -> bool {
    if value.len() != 10 {
        return false;
    }
    let b = value.as_bytes();
    b[4] == b'-'
        && b[7] == b'-'
        && b[0..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
}

fn today_ymd() -> String {
    match std::process::Command::new("date").arg("+%Y-%m-%d").output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "1970-01-01".to_string(),
    }
}

fn set_current_file(app: &AppPaths, file: &TaskFile) -> io::Result<()> {
    fs::write(&app.current_file, task_file_key(file))
}

fn parse_task_file(value: &str) -> Option<TaskFile> {
    if let Some(core) = CoreFile::from_str(value) {
        Some(TaskFile::Core(core))
    } else if is_project_arg(value) {
        Some(TaskFile::Project(project_name_from_arg(value).to_string()))
    } else {
        None
    }
}

fn parse_done_file(value: &str) -> Option<DoneSelection> {
    if value == "all" {
        Some(DoneSelection::All)
    } else {
        parse_task_file(value).map(DoneSelection::File)
    }
}

fn is_project_arg(value: &str) -> bool {
    value.strip_prefix("projects/").is_some_and(|name| {
        !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
    })
}

fn project_name_from_arg(value: &str) -> &str {
    value.strip_prefix("projects/").unwrap_or("")
}

fn task_file_path(app: &AppPaths, file: &TaskFile) -> PathBuf {
    match file {
        TaskFile::Core(core) => app.root.join(core.filename()),
        TaskFile::Project(name) => app.root.join("projects").join(format!("{name}.md")),
    }
}

fn task_file_filename(file: &TaskFile) -> String {
    match file {
        TaskFile::Core(core) => core.filename().to_string(),
        TaskFile::Project(name) => format!("projects/{name}.md"),
    }
}

fn task_file_label(file: &TaskFile) -> String {
    match file {
        TaskFile::Core(core) => core.label().to_string(),
        TaskFile::Project(name) => format!("Project: {name}"),
    }
}

fn task_file_key(file: &TaskFile) -> String {
    match file {
        TaskFile::Core(core) => match core {
            CoreFile::Inbox => "inbox".to_string(),
            CoreFile::Today => "today".to_string(),
            CoreFile::Waiting => "waiting".to_string(),
            CoreFile::Someday => "someday".to_string(),
        },
        TaskFile::Project(name) => format!("projects/{name}"),
    }
}

fn project_files(app: &AppPaths) -> io::Result<Vec<TaskFile>> {
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(app.root.join("projects"))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            if let Some(stem) = path.file_stem().and_then(|v| v.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names.into_iter().map(TaskFile::Project).collect())
}

fn all_task_files(app: &AppPaths) -> io::Result<Vec<TaskFile>> {
    let mut files = vec![
        TaskFile::Core(CoreFile::Inbox),
        TaskFile::Core(CoreFile::Today),
        TaskFile::Core(CoreFile::Waiting),
        TaskFile::Core(CoreFile::Someday),
    ];
    files.extend(project_files(app)?);
    Ok(files)
}

struct OverdueTask {
    due: String,
    file: String,
    line: String,
    descriptions: Vec<String>,
}

fn stale_inbox_count(inbox_path: &Path, lines: &[String]) -> Result<usize, String> {
    let metadata = fs::metadata(inbox_path).map_err(|e| format!("failed to stat inbox.md: {e}"))?;
    let modified = metadata
        .modified()
        .map_err(|e| format!("failed to read inbox mtime: {e}"))?;
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(3 * 24 * 60 * 60))
        .ok_or_else(|| "failed to compute stale cutoff".to_string())?;
    if modified > cutoff {
        return Ok(0);
    }

    Ok(task_line_indices(lines)
        .into_iter()
        .filter(|idx| is_open_task_line(&lines[*idx]) && extract_due_date(&lines[*idx]).is_none())
        .count())
}

fn move_task_block(
    app: &AppPaths,
    source: &TaskFile,
    remove_idx: usize,
    target: &TaskFile,
    label: String,
) -> Result<(), String> {
    let source_path = task_file_path(app, source);
    let mut source_lines = read_lines(&source_path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(source)))?;
    let remove_end = task_block_end(&source_lines, remove_idx);
    let moved_block: Vec<String> = source_lines.drain(remove_idx..remove_end).collect();
    let line = moved_block
        .first()
        .cloned()
        .ok_or_else(|| "internal error: empty task block".to_string())?;

    write_lines(&source_path, &source_lines)
        .map_err(|e| format!("failed to update {}: {e}", task_file_filename(source)))?;

    let target_path = task_file_path(app, target);
    append_lines(&target_path, &moved_block)
        .map_err(|e| format!("failed to write target file: {e}"))?;
    set_current_file(app, target).map_err(|e| e.to_string())?;

    println!(
        "{C_GREEN}Moved{C_RESET} task {} from {}{}{} to {}{}{}:",
        label,
        C_CYAN,
        task_file_filename(source),
        C_RESET,
        C_CYAN,
        task_file_filename(target),
        C_RESET
    );
    println!("  {C_BLUE}•{C_RESET} {}", line);
    Ok(())
}

fn frontmatter_bounds(lines: &[String]) -> Option<(usize, usize)> {
    if lines.first().is_some_and(|line| line.trim() == "---") {
        for (i, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "---" {
                return Some((0, i + 1));
            }
        }
    }
    None
}

fn frontmatter_body_start(lines: &[String]) -> usize {
    frontmatter_bounds(lines).map_or(0, |(_, end)| end)
}

fn parse_frontmatter(lines: &[String]) -> (Vec<(String, String)>, usize) {
    let Some((start, end)) = frontmatter_bounds(lines) else {
        return (Vec::new(), 0);
    };
    let mut fields = Vec::new();
    for line in &lines[start + 1..end - 1] {
        if let Some((k, v)) = line.split_once(':') {
            fields.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    (fields, end)
}

fn with_frontmatter(frontmatter: &[(String, String)], body: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if !frontmatter.is_empty() {
        out.push("---".to_string());
        for (k, v) in frontmatter {
            out.push(format!("{k}: {v}"));
        }
        out.push("---".to_string());
    }
    out.extend_from_slice(body);
    out
}

fn parse_meta_set_value(value: &str) -> Result<(String, String), String> {
    let (key, val) = value
        .split_once(':')
        .ok_or("--set must be in \"key: value\" format")?;
    let key = key.trim().to_string();
    let val = val.trim().to_string();
    if key.is_empty() {
        return Err("--set key must not be empty".to_string());
    }
    Ok((key, val))
}

fn normalize_task_id(value: &str) -> Result<String, String> {
    let id = value.trim().to_ascii_lowercase();
    if id.len() == 6 && id.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(id)
    } else {
        Err("task id must be a 6-character hex string".to_string())
    }
}

fn extract_task_id(line: &str) -> Option<String> {
    let (_, tail) = line.rsplit_once("🆔 ")?;
    let token = tail.split_whitespace().next()?;
    if token.len() == 6 && token.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(token.to_ascii_lowercase())
    } else {
        None
    }
}

fn parse_display_task_line(line: &str) -> Option<DisplayTaskLine> {
    let (done, body) = if let Some(rest) = line.strip_prefix("- [x] ") {
        (true, rest)
    } else if let Some(rest) = line.strip_prefix("- [ ] ") {
        (false, rest)
    } else {
        return None;
    };

    let tokens: Vec<&str> = body.split_whitespace().collect();
    let mut text_tokens: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    let mut due: Option<String> = None;
    let mut alarm: Option<AlarmSpec> = None;
    let mut done_at: Option<String> = None;

    let mut i = 0usize;
    while i < tokens.len() {
        let token = tokens[i];
        if let Some(tag) = token.strip_prefix('#') {
            if !tag.is_empty() {
                tags.push(tag.to_string());
            }
            i += 1;
            continue;
        }

        if token == "📅" && i + 1 < tokens.len() && is_valid_date(tokens[i + 1]) {
            due = Some(tokens[i + 1].to_string());
            i += 2;
            continue;
        }
        if token == "⏰" && i + 1 < tokens.len() {
            let value = tokens[i + 1];
            if is_valid_hhmm(value) {
                alarm = Some(AlarmSpec::Time(value.to_string()));
                i += 2;
                continue;
            }
            if is_valid_datetime(value) {
                alarm = Some(AlarmSpec::DateTime(value.to_string()));
                i += 2;
                continue;
            }
        }
        if token == "✅" && i + 1 < tokens.len() && is_valid_date(tokens[i + 1]) {
            done_at = Some(tokens[i + 1].to_string());
            i += 2;
            continue;
        }
        if token == "🆔" && i + 1 < tokens.len() {
            i += 2;
            continue;
        }
        if (token == "🔁" || token == "👤") && i + 1 < tokens.len() {
            i += 2;
            continue;
        }

        text_tokens.push(token.to_string());
        i += 1;
    }

    Some(DisplayTaskLine {
        done,
        text: text_tokens.join(" ").trim().to_string(),
        tags,
        due,
        alarm,
        done_at,
    })
}

fn alarm_date_part(alarm: Option<&AlarmSpec>) -> Option<&str> {
    match alarm {
        Some(AlarmSpec::DateTime(value)) => value.split_once('T').map(|(d, _)| d),
        _ => None,
    }
}

fn alarm_time_part(alarm: Option<&AlarmSpec>) -> Option<&str> {
    match alarm {
        Some(AlarmSpec::Time(value)) => Some(value.as_str()),
        Some(AlarmSpec::DateTime(value)) => value.split_once('T').map(|(_, t)| t),
        None => None,
    }
}

fn is_valid_hhmm(value: &str) -> bool {
    if value.len() != 5 {
        return false;
    }
    let b = value.as_bytes();
    if b[2] != b':' {
        return false;
    }
    if !b[0..2].iter().all(u8::is_ascii_digit) || !b[3..5].iter().all(u8::is_ascii_digit) {
        return false;
    }
    let hh = value[0..2].parse::<u32>().ok();
    let mm = value[3..5].parse::<u32>().ok();
    matches!(hh, Some(h) if h < 24) && matches!(mm, Some(m) if m < 60)
}

fn is_valid_datetime(value: &str) -> bool {
    let Some((date, time)) = value.split_once('T') else {
        return false;
    };
    is_valid_date(date) && is_valid_hhmm(time)
}

fn human_date_label(date: &str) -> String {
    let Some((y, m, d)) = parse_ymd(date) else {
        return date.to_string();
    };
    let Some((ty, tm, td)) = parse_ymd(&today_ymd()) else {
        return date.to_string();
    };
    let diff = days_from_civil(y, m, d) - days_from_civil(ty, tm, td);
    if diff == 0 {
        return "idag".to_string();
    }
    if diff == 1 {
        return "imorgon".to_string();
    }
    if diff == -1 {
        return "igår".to_string();
    }

    let weekdays = ["sön", "mån", "tis", "ons", "tor", "fre", "lör"];
    let months = [
        "jan", "feb", "mar", "apr", "maj", "jun", "jul", "aug", "sep", "okt", "nov", "dec",
    ];
    let weekday_idx = ((days_from_civil(y, m, d) + 4).rem_euclid(7)) as usize;
    let month_idx = (m.saturating_sub(1) as usize).min(11);
    format!("{} {} {}", weekdays[weekday_idx], d, months[month_idx])
}

fn parse_ymd(value: &str) -> Option<(i32, u32, u32)> {
    if !is_valid_date(value) {
        return None;
    }
    let year = value[0..4].parse::<i32>().ok()?;
    let month = value[5..7].parse::<u32>().ok()?;
    let day = value[8..10].parse::<u32>().ok()?;
    if (1..=12).contains(&month) && (1..=31).contains(&day) {
        Some((year, month, day))
    } else {
        None
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = (yoe + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (mp + if mp < 10 { 3 } else { -9 }) as u32;
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn add_days_to_date(date: &str, days: i64) -> Option<String> {
    let (y, m, d) = parse_ymd(date)?;
    let next = days_from_civil(y, m, d) + days;
    let (ny, nm, nd) = civil_from_days(next);
    Some(format!("{ny:04}-{nm:02}-{nd:02}"))
}

fn next_weekday_date(date: &str, target_weekday: i64) -> Option<String> {
    let (y, m, d) = parse_ymd(date)?;
    let current_days = days_from_civil(y, m, d);
    let current_weekday = (current_days + 4).rem_euclid(7);
    let mut delta = (target_weekday - current_weekday).rem_euclid(7);
    if delta == 0 {
        delta = 7;
    }
    add_days_to_date(date, delta)
}

fn alarm_trigger_time(task: &DisplayTaskLine) -> Option<String> {
    let alarm = task.alarm.as_ref()?;
    match alarm {
        AlarmSpec::DateTime(value) => {
            if is_valid_datetime(value) {
                Some(value.clone())
            } else {
                None
            }
        }
        AlarmSpec::Time(hhmm) => {
            let date = task.due.clone().unwrap_or_else(today_ymd);
            if is_valid_date(&date) && is_valid_hhmm(hhmm) {
                Some(format!("{date}T{hhmm}"))
            } else {
                None
            }
        }
    }
}

fn generate_new_task_id(app: &AppPaths) -> Result<String, String> {
    let files = all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))?;
    let mut taken: HashSet<String> = HashSet::new();
    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        for idx in task_line_indices(&lines) {
            if let Some(id) = extract_task_id(&lines[idx]) {
                taken.insert(id);
            }
        }
    }

    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for attempt in 0u64..5000 {
        let candidate = format!(
            "{:06x}",
            ((seed.wrapping_add(attempt * 977)) & 0x00ff_ffff) as u32
        );
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err("failed to generate unique task id".to_string())
}

fn selection_files(app: &AppPaths, selection: &DoneSelection) -> Result<Vec<TaskFile>, String> {
    match selection {
        DoneSelection::File(file) => Ok(vec![file.clone()]),
        DoneSelection::All => {
            all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))
        }
    }
}

fn find_task_by_id(
    app: &AppPaths,
    task_id: &str,
    selection: Option<&DoneSelection>,
) -> Result<Option<(TaskFile, usize)>, String> {
    let files = if let Some(selection) = selection {
        selection_files(app, selection)?
    } else {
        all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))?
    };
    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        for idx in task_line_indices(&lines) {
            if extract_task_id(&lines[idx]).as_deref() == Some(task_id) {
                return Ok(Some((file, idx)));
            }
        }
    }
    Ok(None)
}

fn parse_task_line(line: &str) -> Option<ParsedTaskLine> {
    let done = if line.starts_with("- [x] ") {
        true
    } else if line.starts_with("- [ ] ") {
        false
    } else {
        return None;
    };

    let mut text = line[6..].to_string();
    let mut id = None;
    if let Some(found_id) = extract_task_id(&text) {
        if let Some((head, _)) = text.rsplit_once(" 🆔 ") {
            text = head.to_string();
            id = Some(found_id);
        }
    }

    let mut due = None;
    if let Some((head, tail)) = text.rsplit_once(" 📅 ") {
        let token = tail.split_whitespace().next().unwrap_or("");
        if is_valid_date(token) {
            due = Some(token.to_string());
            text = head.to_string();
        }
    }

    let mut done_at = None;
    if let Some((head, tail)) = text.rsplit_once(" ✅ ") {
        let token = tail.split_whitespace().next().unwrap_or("");
        if is_valid_date(token) {
            done_at = Some(token.to_string());
            text = head.to_string();
        }
    }

    let mut recurrence = None;
    if let Some((head, tail)) = text.rsplit_once(" 🔁 ") {
        let token = tail.split_whitespace().next().unwrap_or("");
        recurrence = match token {
            "daily" => Some(Recurrence::Daily),
            "weekly" => Some(Recurrence::Weekly),
            "monthly" => Some(Recurrence::Monthly),
            "weekdays" => Some(Recurrence::Weekdays),
            _ => None,
        };
        if recurrence.is_some() {
            text = head.to_string();
        }
    }

    let tags = text
        .split_whitespace()
        .filter_map(|token| token.strip_prefix('#'))
        .filter(|tag| !tag.is_empty())
        .map(|tag| tag.to_string())
        .collect();

    Some(ParsedTaskLine {
        done,
        text: text.trim().to_string(),
        tags,
        due,
        id,
        recurrence,
        done_at,
    })
}

fn collect_tasks_for_file(app: &AppPaths, file: &TaskFile) -> Result<Vec<TaskJsonRecord>, String> {
    let path = task_file_path(app, file);
    let lines = read_lines(&path)
        .map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
    let mut tasks = Vec::new();
    for idx in task_line_indices(&lines) {
        let line = &lines[idx];
        if let Some(parsed) = parse_task_line(line) {
            tasks.push(TaskJsonRecord {
                id: parsed.id,
                done: parsed.done,
                text: parsed.text,
                tags: parsed.tags,
                due: parsed.due,
                file: task_file_filename(file),
                description: collect_descriptions(&lines, idx)
                    .into_iter()
                    .map(|d| d.trim_start_matches("  ").to_string())
                    .collect(),
                subtasks: collect_subtasks(&lines, idx)
                    .into_iter()
                    .map(|s| SubtaskJsonRecord {
                        done: s.done,
                        text: s.text,
                        done_at: s.done_at,
                    })
                    .collect(),
            });
        }
    }
    Ok(tasks)
}

fn tasks_for_list_selection(
    app: &AppPaths,
    selection: &ListSelection,
) -> Result<Vec<TaskJsonRecord>, String> {
    match selection {
        ListSelection::Inbox => collect_tasks_for_file(app, &TaskFile::Core(CoreFile::Inbox)),
        ListSelection::Today => collect_tasks_for_file(app, &TaskFile::Core(CoreFile::Today)),
        ListSelection::Projects => {
            let mut out = Vec::new();
            for project in
                project_files(app).map_err(|e| format!("failed to read projects/: {e}"))?
            {
                out.extend(collect_tasks_for_file(app, &project)?);
            }
            Ok(out)
        }
        ListSelection::Project(name) => {
            collect_tasks_for_file(app, &TaskFile::Project(name.clone()))
        }
        ListSelection::All => {
            let mut out = Vec::new();
            for file in all_task_files(app).map_err(|e| format!("failed to read files: {e}"))? {
                out.extend(collect_tasks_for_file(app, &file)?);
            }
            Ok(out)
        }
        ListSelection::Tag(tag) => {
            let mut out = Vec::new();
            let normalized = tag.trim_start_matches('#');
            for file in all_task_files(app).map_err(|e| format!("failed to read files: {e}"))? {
                for task in collect_tasks_for_file(app, &file)? {
                    if task.tags.iter().any(|t| t == normalized) {
                        out.push(task);
                    }
                }
            }
            Ok(out)
        }
    }
}

fn collect_all_alarms(app: &AppPaths) -> Result<Vec<AlarmRecord>, String> {
    let files = all_task_files(app).map_err(|e| format!("failed to read files: {e}"))?;
    let mut out: Vec<AlarmRecord> = Vec::new();
    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path)
            .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        for idx in task_line_indices(&lines) {
            let line = &lines[idx];
            let Some(parsed) = parse_display_task_line(line) else {
                continue;
            };
            if parsed.done {
                continue;
            }
            let Some(when) = alarm_trigger_time(&parsed) else {
                continue;
            };
            out.push(AlarmRecord {
                when,
                file: file.clone(),
                line: line.clone(),
                descriptions: collect_descriptions(&lines, idx)
                    .into_iter()
                    .map(|d| d.trim_start_matches("  ").to_string())
                    .collect(),
            });
        }
    }
    out.sort_by(|a, b| a.when.cmp(&b.when));
    Ok(out)
}

fn now_ymd_hm() -> String {
    match Command::new("date").arg("+%Y-%m-%dT%H:%M").output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "1970-01-01T00:00".to_string(),
    }
}

fn trigger_alarm_handler(handler: &str, alarm: &AlarmRecord) -> Result<(), String> {
    if handler == "stdout" {
        print_alarm_event_json(alarm);
        return Ok(());
    }
    if let Some(command) = handler.strip_prefix("exec:") {
        return run_exec_handler(command, alarm);
    }
    if let Some(url) = handler.strip_prefix("webhook:") {
        return run_webhook_handler(url, alarm);
    }
    Err("watch.handler must be stdout, exec:<path>, or webhook:<url>".to_string())
}

fn print_alarm_event_json(alarm: &AlarmRecord) {
    let parsed = parse_display_task_line(&alarm.line);
    let done = parsed.as_ref().is_some_and(|p| p.done);
    let text = parsed
        .as_ref()
        .map_or_else(|| alarm.line.clone(), |p| p.text.clone());
    let tags = parsed.map_or_else(Vec::new, |p| p.tags);
    let due = parse_display_task_line(&alarm.line).and_then(|p| p.due);
    let payload = format!(
        "{{\"event\":\"alarm\",\"task\":{{\"done\":{},\"text\":{},\"tags\":{},\"due\":{},\"alarm\":{},\"file\":{},\"description\":{}}}}}",
        done,
        json_string(&text),
        json_string_array(&tags),
        due.as_ref()
            .map_or_else(|| "null".to_string(), |v| json_string(v)),
        json_string(&alarm.when),
        json_string(&task_file_filename(&alarm.file)),
        json_string_array(&alarm.descriptions)
    );
    println!("{payload}");
}

fn run_exec_handler(command: &str, alarm: &AlarmRecord) -> Result<(), String> {
    let expanded = expand_tilde(command);
    let output = Command::new("sh")
        .arg("-c")
        .arg(&expanded)
        .env("DODO_EVENT", "alarm")
        .env("DODO_ALARM_WHEN", &alarm.when)
        .env("DODO_TASK_FILE", task_file_filename(&alarm.file))
        .env("DODO_TASK_LINE", &alarm.line)
        .output()
        .map_err(|e| format!("failed to run exec handler: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "exec handler failed with status {}",
            output.status.code().unwrap_or(1)
        ))
    }
}

fn run_webhook_handler(url: &str, alarm: &AlarmRecord) -> Result<(), String> {
    let payload = format!(
        "{{\"event\":\"alarm\",\"task\":{{\"line\":{},\"file\":{},\"when\":{}}}}}",
        json_string(&alarm.line),
        json_string(&task_file_filename(&alarm.file)),
        json_string(&alarm.when)
    );
    let status = Command::new("curl")
        .arg("-fsS")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(payload)
        .arg(url)
        .status()
        .map_err(|e| format!("failed to run webhook handler: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("webhook handler failed with status {:?}", status.code()))
    }
}

fn expand_tilde(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    value.to_string()
}

fn default_config() -> AppConfig {
    AppConfig {
        watch: WatchConfig {
            handler: "stdout".to_string(),
            interval: 60,
        },
    }
}

fn read_app_config(app: &AppPaths) -> Result<AppConfig, String> {
    if !app.config_file.exists() {
        return Ok(default_config());
    }
    let content = fs::read_to_string(&app.config_file)
        .map_err(|e| format!("failed to read config: {e}"))?;
    let mut cfg = default_config();
    let mut in_watch = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_watch = line == "[watch]";
            continue;
        }
        if !in_watch {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let value = v.trim().trim_matches('"');
        match key {
            "handler" => cfg.watch.handler = value.to_string(),
            "interval" => {
                if let Ok(parsed) = value.parse::<u64>() {
                    cfg.watch.interval = parsed.max(1);
                }
            }
            _ => {}
        }
    }
    Ok(cfg)
}

fn write_app_config(app: &AppPaths, cfg: &AppConfig) -> Result<(), String> {
    let content = format!(
        "[watch]\nhandler = {}\ninterval = {}\n",
        json_string(&cfg.watch.handler),
        cfg.watch.interval.max(1)
    );
    fs::write(&app.config_file, content).map_err(|e| format!("failed to write config: {e}"))
}

fn set_config_value(app: &AppPaths, key: &str, value: ConfigValue) -> Result<(), String> {
    let mut cfg = read_app_config(app)?;
    match (key, value) {
        ("watch.handler", ConfigValue::Text(v)) => cfg.watch.handler = v,
        ("watch.interval", ConfigValue::Number(v)) => cfg.watch.interval = v.max(1),
        _ => return Err("unsupported config key".to_string()),
    }
    write_app_config(app, &cfg)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_string_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    out
}

fn print_tasks_json(tasks: &[TaskJsonRecord]) {
    println!("[");
    for (i, task) in tasks.iter().enumerate() {
        let comma = if i + 1 == tasks.len() { "" } else { "," };
        let id_json = task
            .id
            .as_ref()
            .map_or_else(|| "null".to_string(), |v| json_string(v));
        let due_json = task
            .due
            .as_ref()
            .map_or_else(|| "null".to_string(), |v| json_string(v));
        println!("  {{");
        println!("    \"id\": {},", id_json);
        println!("    \"done\": {},", task.done);
        println!("    \"text\": {},", json_string(&task.text));
        println!("    \"tags\": {},", json_string_array(&task.tags));
        println!("    \"due\": {},", due_json);
        println!("    \"file\": {},", json_string(&task.file));
        println!(
            "    \"description\": {},",
            json_string_array(&task.description)
        );
        println!("    \"subtasks\": [");
        for (sub_idx, subtask) in task.subtasks.iter().enumerate() {
            let sub_comma = if sub_idx + 1 == task.subtasks.len() {
                ""
            } else {
                ","
            };
            let done_at_json = subtask
                .done_at
                .as_ref()
                .map_or_else(|| "null".to_string(), |v| json_string(v));
            println!("      {{");
            println!("        \"done\": {},", subtask.done);
            println!("        \"text\": {},", json_string(&subtask.text));
            println!("        \"done_at\": {}", done_at_json);
            println!("      }}{}", sub_comma);
        }
        println!("    ]");
        println!("  }}{}", comma);
    }
    println!("]");
}

fn print_status_json(
    total_open: usize,
    overdue: usize,
    project_count: usize,
    stale_inbox: usize,
    open_by_file: &[(String, usize)],
) {
    println!("{{");
    println!("  \"open\": {},", total_open);
    println!("  \"overdue\": {},", overdue);
    println!("  \"projects\": {},", project_count);
    println!("  \"stale_inbox\": {},", stale_inbox);
    println!("  \"open_by_file\": {{");
    for (idx, (file, count)) in open_by_file.iter().enumerate() {
        let comma = if idx + 1 == open_by_file.len() {
            ""
        } else {
            ","
        };
        println!("    {}: {}{}", json_string(file), count, comma);
    }
    println!("  }}");
    println!("}}");
}

fn help_text() -> &'static str {
    "dodo - local markdown task manager\n\
\n\
Commands:\n\
  dodo add \"Task description\" [--file today|inbox|waiting|someday|projects/<name>] [--section NAME] [--due DATE] [--tag TAG]\n\
  dodo list [--file today|inbox|projects|projects/<name>|all] [--tag TAG] [--raw] [--json]\n\
  dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all\n\
  dodo done --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]\n\
  dodo subtask add <parent-id> \"text\"\n\
  dodo subtask done <parent-id> <sub-num>\n\
  dodo subtask list <parent-id>\n\
  dodo section add \"Name\" --file today|inbox|waiting|someday|projects/<name>\n\
  dodo delete <id> --file inbox|today|waiting|someday|projects/<name>\n\
  dodo delete --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]\n\
  dodo clean --file inbox|today|waiting|someday|projects/<name>|all\n\
  dodo edit <id> --file inbox|today|waiting|someday|projects/<name> \"New task text\"\n\
  dodo edit --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all] \"New task text\"\n\
  dodo describe <id> --file inbox|today|waiting|someday|projects/<name> \"Description text\"\n\
  dodo note <id> --file inbox|today|waiting|someday|projects/<name> \"Description text\"\n\
  dodo inbox [--json]\n\
  dodo today [--json]\n\
  dodo move <id> <file>\n\
  dodo move --id <6-hex> <file>\n\
  dodo meta --file inbox|today|waiting|someday|projects/<name> [--set \"key: value\"]\n\
  dodo alarms\n\
  dodo watch\n\
  dodo config get watch.handler|watch.interval\n\
  dodo config set watch.handler|watch.interval <value>\n\
  dodo status [--json]\n\
  dodo overdue [--json]\n"
}

fn print_help() {
    println!("{}", help_text());
}
