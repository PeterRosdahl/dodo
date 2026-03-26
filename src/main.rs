use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
        "inbox" => cmd_show_single(&app, CoreFile::Inbox),
        "today" => cmd_show_single(&app, CoreFile::Today),
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
                    "--file must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                saw_file = true;
                if saw_tag {
                    return Err("--file and --tag cannot be used together".to_string());
                }
                i += 1;
                let value = args.get(i).ok_or("--file requires a value")?;
                selection = match value.as_str() {
                    "inbox" => ListSelection::Inbox,
                    "today" => ListSelection::Today,
                    "projects" => ListSelection::Projects,
                    "all" => ListSelection::All,
                    _ if is_project_arg(value) => {
                        ListSelection::Project(project_name_from_arg(value).to_string())
                    }
                    _ => return Err("--file must be one of: inbox, today, projects, projects/<name>, all".to_string()),
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

    match selection {
        ListSelection::Inbox => {
            print_tasks_for(app, CoreFile::Inbox)?;
            set_current_file(app, &TaskFile::Core(CoreFile::Inbox)).map_err(|e| e.to_string())?;
        }
        ListSelection::Today => {
            print_tasks_for(app, CoreFile::Today)?;
            set_current_file(app, &TaskFile::Core(CoreFile::Today)).map_err(|e| e.to_string())?;
        }
        ListSelection::Projects => {
            print_all_project_tasks(app)?;
        }
        ListSelection::Project(name) => {
            let file = TaskFile::Project(name);
            print_tasks_for_task_file(app, &file)?;
            set_current_file(app, &file).map_err(|e| e.to_string())?;
        }
        ListSelection::All => {
            print_tasks_all_global(app)?;
        }
        ListSelection::Tag(tag) => {
            print_tasks_by_tag(app, &tag)?;
        }
    }

    Ok(())
}

fn cmd_done(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.len() != 3 {
        return Err("usage: dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all".to_string());
    }

    let id: usize = args[0]
        .parse()
        .map_err(|_| "<id> must be a positive number".to_string())?;
    if id == 0 {
        return Err("<id> must be >= 1".to_string());
    }

    if args[1] != "--file" {
        return Err("usage: dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all".to_string());
    }
    let selection = parse_done_file(&args[2]).ok_or_else(|| {
        "--file must be one of: inbox, today, waiting, someday, projects/<name>, all".to_string()
    })?;

    let (file, idx, label_id) = match selection {
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
            let all_files = all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))?;
            let mut global = 0usize;
            let mut found: Option<(TaskFile, usize)> = None;
            for file in all_files {
                let path = task_file_path(app, &file);
                let lines = read_lines(&path)
                    .map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
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
            let (file, idx) = found.ok_or_else(|| format!("task id {} not found in --file all", id))?;
            (file, idx, format!("global #{id}"))
        }
    };

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
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
        C_RESET
        ,
        label_id
    );
    println!("  {C_BLUE}•{C_RESET} {}", updated);
    Ok(())
}

fn cmd_delete(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.len() != 3 {
        return Err("usage: dodo delete <id> --file inbox|today|waiting|someday|projects/<name>".to_string());
    }

    let id: usize = args[0]
        .parse()
        .map_err(|_| "<id> must be a positive number".to_string())?;
    if id == 0 {
        return Err("<id> must be >= 1".to_string());
    }

    if args[1] != "--file" {
        return Err("usage: dodo delete <id> --file inbox|today|waiting|someday|projects/<name>".to_string());
    }

    let file = parse_task_file(&args[2]).ok_or_else(|| {
        "--file must be one of: inbox, today, waiting, someday, projects/<name>".to_string()
    })?;

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
    let task_indices = task_line_indices(&lines);
    if id > task_indices.len() {
        return Err(format!(
            "task id {} not found in {} ({} tasks)",
            id,
            task_file_filename(&file),
            task_indices.len()
        ));
    }

    let remove_idx = task_indices[id - 1];
    let remove_end = task_block_end(&lines, remove_idx);
    let removed_line = lines[remove_idx].clone();
    lines.drain(remove_idx..remove_end);

    write_lines(&path, &lines).map_err(|e| format!("failed to update {}: {e}", task_file_filename(&file)))?;
    println!("Deleted from {} (#{id}): {}", task_file_filename(&file), removed_line);
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
        DoneSelection::All => all_task_files(app).map_err(|e| format!("failed to collect files: {e}"))?,
    };

    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
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
    if new_text.is_empty() {
        return Err("new task text must not be empty".to_string());
    }

    let path = task_file_path(app, &file);
    let mut lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
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
    let original = &lines[idx];
    let prefix = if original.starts_with("- [x] ") {
        "- [x] "
    } else {
        "- [ ] "
    };
    let updated = format!("{prefix}{new_text}");
    lines[idx] = updated.clone();
    write_lines(&path, &lines).map_err(|e| format!("failed to update task: {e}"))?;

    println!(
        "{C_GREEN}Edited{C_RESET} in {}{}{} (#{id}):",
        C_CYAN,
        task_file_filename(&file),
        C_RESET
    );
    println!("  {C_BLUE}•{C_RESET} {}", updated);
    Ok(())
}

fn cmd_move(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("usage: dodo move <id> <file>".to_string());
    }

    let id: usize = args[0]
        .parse()
        .map_err(|_| "<id> must be a positive number".to_string())?;
    if id == 0 {
        return Err("<id> must be >= 1".to_string());
    }

    let target = parse_task_file(&args[1])
        .ok_or_else(|| "<file> must be one of: inbox, today, waiting, someday, projects/<name>".to_string())?;

    let inbox_path = app.root.join(CoreFile::Inbox.filename());
    let mut inbox_lines = read_lines(&inbox_path).map_err(|e| format!("failed to read inbox: {e}"))?;
    let inbox_task_indices = task_line_indices(&inbox_lines);

    if id > inbox_task_indices.len() {
        return Err(format!("task id {} not found in inbox ({})", id, inbox_task_indices.len()));
    }

    let remove_idx = inbox_task_indices[id - 1];
    let remove_end = task_block_end(&inbox_lines, remove_idx);
    let moved_block: Vec<String> = inbox_lines.drain(remove_idx..remove_end).collect();
    let line = moved_block
        .first()
        .cloned()
        .ok_or_else(|| "internal error: empty task block".to_string())?;

    write_lines(&inbox_path, &inbox_lines).map_err(|e| format!("failed to update inbox: {e}"))?;

    let target_path = task_file_path(app, &target);
    append_lines(&target_path, &moved_block).map_err(|e| format!("failed to write target file: {e}"))?;
    set_current_file(app, &target).map_err(|e| e.to_string())?;

    println!(
        "{C_GREEN}Moved{C_RESET} task #{id} from {}inbox.md{} to {}{}{}:",
        C_CYAN,
        C_RESET,
        C_CYAN,
        task_file_filename(&target),
        C_RESET
    );
    println!("  {C_BLUE}•{C_RESET} {}", line);
    Ok(())
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
    let mut lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
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

fn cmd_show_single(app: &AppPaths, file: CoreFile) -> Result<(), String> {
    print_tasks_for(app, file)?;
    set_current_file(app, &TaskFile::Core(file)).map_err(|e| e.to_string())?;
    Ok(())
}

fn cmd_status(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: dodo status".to_string());
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

    println!("{C_BOLD}{C_CYAN}Status{C_RESET}");
    println!("{C_BOLD}{C_CYAN}Open Tasks By File{C_RESET}");
    for file in &files {
        let path = task_file_path(app, file);
        let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
        let open_count = lines.iter().filter(|line| is_open_task_line(line)).count();
        total_open += open_count;
        println!(
            "  {C_BLUE}•{C_RESET} {}{}{}: {}",
            C_CYAN,
            task_file_filename(file),
            C_RESET,
            open_count
        );

        for (idx, line) in lines.iter().enumerate() {
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

    println!();
    println!("{C_BOLD}{C_CYAN}Overdue Tasks{C_RESET}");
    if overdue_tasks.is_empty() {
        println!("  {C_GREEN}(none){C_RESET}");
    } else {
        overdue_tasks.sort_by(|a, b| a.due.cmp(&b.due));
        for task in &overdue_tasks {
            println!(
                "  {C_RED}• [{}] {} ({}){C_RESET}",
                task.file, task.line, task.due
            );
            for description in &task.descriptions {
                println!("      {C_DIM}{}{C_RESET}", description.trim_start_matches("  "));
            }
        }
    }

    let inbox_path = app.root.join(CoreFile::Inbox.filename());
    let inbox_lines = read_lines(&inbox_path).map_err(|e| format!("failed to read inbox.md: {e}"))?;
    let stale_inbox_count = stale_inbox_count(&inbox_path, &inbox_lines)?;
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

    Ok(())
}

fn cmd_overdue(app: &AppPaths, args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: dodo overdue".to_string());
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
        let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
        for (idx, line) in lines.iter().enumerate() {
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
                println!("      {C_DIM}{}{C_RESET}", description.trim_start_matches("  "));
            }
        }
    }

    Ok(())
}

fn print_tasks_for(app: &AppPaths, file: CoreFile) -> Result<(), String> {
    let path = app.root.join(file.filename());
    let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", file.filename()))?;

    println!("{C_BOLD}{C_CYAN}{} ({}){C_RESET}", file.label(), file.filename());

    let shown = print_numbered_tasks(&lines, 0);

    if shown == 0 {
        println!("  {C_BLUE}(no tasks){C_RESET}");
    }

    Ok(())
}

fn print_tasks_for_task_file(app: &AppPaths, file: &TaskFile) -> Result<(), String> {
    let path = task_file_path(app, file);
    let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;

    println!(
        "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
        task_file_label(file),
        task_file_filename(file)
    );

    let shown = print_numbered_tasks(&lines, 0);

    if shown == 0 {
        println!("  {C_BLUE}(no tasks){C_RESET}");
    }

    Ok(())
}

fn print_all_project_tasks(app: &AppPaths) -> Result<(), String> {
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
        print_tasks_for_task_file(app, project)?;
    }
    Ok(())
}

fn print_tasks_all_global(app: &AppPaths) -> Result<(), String> {
    let files = all_task_files(app).map_err(|e| format!("failed to read files: {e}"))?;
    let mut shown = 0usize;
    for (file_idx, file) in files.iter().enumerate() {
        if file_idx > 0 {
            println!();
        }
        let path = task_file_path(app, file);
        let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(file)))?;
        println!(
            "{C_BOLD}{C_CYAN}{} ({}){C_RESET}",
            task_file_label(file),
            task_file_filename(file)
        );
        let previous = shown;
        shown = print_numbered_tasks(&lines, shown);
        let any = shown > previous;
        if !any {
            println!("  {C_BLUE}(no tasks){C_RESET}");
        }
    }
    Ok(())
}

fn print_tasks_by_tag(app: &AppPaths, tag: &str) -> Result<(), String> {
    let files = all_task_files(app).map_err(|e| format!("failed to read files: {e}"))?;
    let needle = format!("#{}", tag.trim_start_matches('#'));
    let mut found_any = false;

    for file in files {
        let path = task_file_path(app, &file);
        let lines = read_lines(&path).map_err(|e| format!("failed to read {}: {e}", task_file_filename(&file)))?;
        let mut matched: Vec<(String, Vec<String>)> = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            if is_task_line(line) && line.contains(&needle) {
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
                println!("      {C_DIM}{}{C_RESET}", description.trim_start_matches("  "));
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
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| if is_task_line(line) { Some(i) } else { None })
        .collect()
}

fn is_task_line(line: &str) -> bool {
    line.starts_with("- [ ] ") || line.starts_with("- [x] ")
}

fn is_description_line(line: &str) -> bool {
    line
        .strip_prefix("  ")
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
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = &lines[idx];
        if line.starts_with("- [x] ") {
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
    for (idx, line) in lines.iter().enumerate() {
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
            println!("      {C_DIM}{}{C_RESET}", description.trim_start_matches("  "));
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
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim().to_string(),
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
    value
        .strip_prefix("projects/")
        .is_some_and(|name| !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains(".."))
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

    Ok(lines
        .iter()
        .filter(|line| is_open_task_line(line) && extract_due_date(line).is_none())
        .count())
}

fn help_text() -> &'static str {
    "dodo - local markdown task manager\n\
\n\
Commands:\n\
  dodo add \"Task description\" [--file today|inbox|waiting|someday|projects/<name>] [--due DATE] [--tag TAG]\n\
  dodo list [--file today|inbox|projects|projects/<name>|all] [--tag TAG]\n\
  dodo done <id> --file inbox|today|waiting|someday|projects/<name>|all\n\
  dodo delete <id> --file inbox|today|waiting|someday|projects/<name>\n\
  dodo clean --file inbox|today|waiting|someday|projects/<name>|all\n\
  dodo edit <id> --file inbox|today|waiting|someday|projects/<name> \"New task text\"\n\
  dodo describe <id> --file inbox|today|waiting|someday|projects/<name> \"Description text\"\n\
  dodo note <id> --file inbox|today|waiting|someday|projects/<name> \"Description text\"\n\
  dodo inbox\n\
  dodo today\n\
  dodo move <id> <file>\n\
  dodo status\n\
  dodo overdue\n"
}

fn print_help() {
    println!("{}", help_text());
}
