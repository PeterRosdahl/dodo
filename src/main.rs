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

struct TaskJsonRecord {
    id: Option<String>,
    done: bool,
    text: String,
    tags: Vec<String>,
    due: Option<String>,
    file: String,
    description: Vec<String>,
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
        "delete" => cmd_delete(&app, &args),
        "clean" => cmd_clean(&app, &args),
        "edit" => cmd_edit(&app, &args),
        "describe" | "note" => cmd_describe(&app, &args),
        "move" => cmd_move(&app, &args),
        "status" => cmd_status(&app, &args),
        "overdue" => cmd_overdue(&app, &args),
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
        fs::write(&current, "inbox")?;
    }

    Ok(AppPaths {
        config_file: root.join("config.toml"),
        root,
        current_file: current,
    })
}

fn cmd_add(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: dodo add \"Task description\" [--file today|inbox|waiting|someday|projects/<name>] [--due YYYY-MM-DD] [--tag TAG]".to_string());
    }

    let description = args[0].trim().to_string();
    if description.is_empty() {
        return Err("task description must not be empty".to_string());
    }

    let mut file = TaskFile::Core(CoreFile::Inbox);
    let mut due: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();

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

    let mut task = format!("- [ ] {}", description);
    for tag in tags {
        task.push(' ');
        task.push('#');
        task.push_str(&tag);
    }
    if let Some(date) = due {
        task.push_str(" 📅 ");
        task.push_str(&date);
    }
    let task_id = generate_new_task_id(app)?;
    task.push_str(" 🆔 ");
    task.push_str(&task_id);

    let path = task_file_path(app, &file);
    append_line(&path, &task).map_err(|e| format!("failed to write task: {e}"))?;
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
                    done: false,
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
            if line.starts_with("- [x]") {
                println!("  {C_GREEN}{:>2}.{C_RESET} {}", id, line);
            } else {
                println!("  {C_YELLOW}{:>2}.{C_RESET} {}", id, line);
            }
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

fn task_line_indices(lines: &[String]) -> Vec<usize> {
    let body_start = frontmatter_body_start(lines);
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            if i >= body_start && is_task_line(line) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn is_task_line(line: &str) -> bool {
    line.starts_with("- [ ] ") || line.starts_with("- [x] ")
}

fn is_description_line(line: &str) -> bool {
    line.strip_prefix("  ")
        .is_some_and(|rest| !rest.starts_with(' '))
        && !is_task_line(line)
}

fn task_block_end(lines: &[String], start_idx: usize) -> usize {
    let mut idx = start_idx + 1;
    while idx < lines.len() && is_description_line(&lines[idx]) {
        idx += 1;
    }
    idx
}

fn collect_descriptions(lines: &[String], task_idx: usize) -> Vec<String> {
    let mut descriptions = Vec::new();
    let mut idx = task_idx + 1;
    while idx < lines.len() && is_description_line(&lines[idx]) {
        descriptions.push(lines[idx].clone());
        idx += 1;
    }
    descriptions
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

fn print_numbered_tasks(lines: &[String], start_id: usize) -> usize {
    let mut shown = start_id;
    let body_start = frontmatter_body_start(lines);
    for (idx, line) in lines.iter().enumerate() {
        if idx < body_start {
            continue;
        }
        if !is_task_line(line) {
            continue;
        }

        shown += 1;
        if line.starts_with("- [x]") {
            println!("  {C_GREEN}{:>2}.{C_RESET} {}", shown, line);
        } else {
            println!("  {C_YELLOW}{:>2}.{C_RESET} {}", shown, line);
        }

        for description in collect_descriptions(lines, idx) {
            println!(
                "      {C_DIM}{}{C_RESET}",
                description.trim_start_matches("  ")
            );
        }
    }
    shown
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
            "    \"description\": {}",
            json_string_array(&task.description)
        );
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
  dodo add \"Task description\" [--file today|inbox|waiting|someday|projects/<name>] [--due DATE] [--tag TAG]\n\
  dodo list [--file today|inbox|projects|projects/<name>|all] [--tag TAG] [--json]\n\
  dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all\n\
  dodo done --id <6-hex> [--file inbox|today|waiting|someday|projects/<name>|all]\n\
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
  dodo status [--json]\n\
  dodo overdue [--json]\n"
}

fn print_help() {
    println!("{}", help_text());
}
