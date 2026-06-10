use crate::config::Config;
use crate::skills;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// ── データ構造 ─────────────────────────────────────────────────

#[derive(Debug)]
struct ParsedRepoUrl {
    clone_url: String,
    subpath: Option<String>,
}

#[derive(Debug)]
struct ImportItem {
    name: String,
    path: PathBuf,
    exists_in_store: bool,
    selected: bool,
}

#[derive(Debug)]
enum ImportAction {
    Added { name: String },
    Overwritten { name: String },
}

// ── 公開エントリポイント ───────────────────────────────────────

pub fn run(url: &str) -> Result<()> {
    let parsed = parse_url(url)?;
    ensure_git_available()?;

    let config = Config::load()?;
    let store_dir = config.get_skills_source_dir();

    println!("Cloning {}...", parsed.clone_url);
    let repo_dir = clone_repo(&parsed.clone_url)?;

    let result = run_with_repo(&repo_dir, &parsed, &store_dir);

    let _ = fs::remove_dir_all(&repo_dir);

    result
}

// ── メインフロー ───────────────────────────────────────────────

fn run_with_repo(repo_dir: &Path, parsed: &ParsedRepoUrl, store_dir: &Path) -> Result<()> {
    let scan_dir = resolve_scan_dir(repo_dir, parsed)?;

    // サブパスがスキルディレクトリそのものを指している場合は1つだけインポート
    if scan_dir.join("SKILL.md").exists() {
        return import_single_skill(&scan_dir, store_dir);
    }

    let skill_dirs = skills::scan_skill_dirs(&scan_dir)
        .context("Failed to scan for skills in the cloned repository")?;

    if skill_dirs.is_empty() {
        anyhow::bail!(
            "No skills found in the repository. \
             A skill is a directory containing a SKILL.md file."
        );
    }

    let mut items = build_import_items(&skill_dirs, store_dir);

    if items.len() == 1 {
        // スキル1つだけの場合は選択UIをスキップ
        return import_single_skill(&items[0].path, store_dir);
    }

    // 複数スキル → 選択UIを表示
    let confirmed = select_skills(&mut items)?;
    if !confirmed {
        println!("Cancelled.");
        return Ok(());
    }

    let selected_dirs: Vec<PathBuf> = items
        .iter()
        .filter(|item| item.selected)
        .map(|item| item.path.clone())
        .collect();

    if selected_dirs.is_empty() {
        println!("No skills selected.");
        return Ok(());
    }

    let effective_dir = match prompt_subdirectory()? {
        Some(sub) => {
            let dir = store_dir.join(&sub);
            fs::create_dir_all(&dir)?;
            println!("Importing into: {}/", sub);
            dir
        }
        None => store_dir.to_path_buf(),
    };

    let actions = import_skills(&selected_dirs, &effective_dir)?;
    print_summary(&actions);

    Ok(())
}

fn resolve_scan_dir(repo_dir: &Path, parsed: &ParsedRepoUrl) -> Result<PathBuf> {
    match &parsed.subpath {
        Some(subpath) => {
            let dir = repo_dir.join(subpath);
            if !dir.exists() {
                anyhow::bail!("Directory '{}' not found in the repository", subpath);
            }
            Ok(dir)
        }
        None => Ok(repo_dir.to_path_buf()),
    }
}

fn import_single_skill(scan_dir: &Path, store_dir: &Path) -> Result<()> {
    let skill_name = scan_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let effective_dir = match prompt_subdirectory()? {
        Some(sub) => {
            let dir = store_dir.join(&sub);
            fs::create_dir_all(&dir)?;
            println!("Importing into: {}/", sub);
            dir
        }
        None => store_dir.to_path_buf(),
    };

    if effective_dir.join(&skill_name).exists() {
        if prompt_overwrite(&skill_name)? {
            let actions = import_skills(&[scan_dir.to_path_buf()], &effective_dir)?;
            print_summary(&actions);
        } else {
            println!("Skipped '{}'.", skill_name);
        }
    } else {
        let actions = import_skills(&[scan_dir.to_path_buf()], &effective_dir)?;
        println!("Found 1 skill: {}", skill_name);
        print_summary(&actions);
    }
    Ok(())
}

fn build_import_items(skill_dirs: &[PathBuf], store_dir: &Path) -> Vec<ImportItem> {
    skill_dirs
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let exists = store_dir.join(&name).exists();
            ImportItem {
                name,
                path: path.clone(),
                exists_in_store: exists,
                selected: !exists, // 新規はデフォルト選択、既存は未選択
            }
        })
        .collect()
}

// ── 選択UI ─────────────────────────────────────────────────────

const COLOR_YELLOW: Color = Color::Rgb(255, 255, 0);
const COLOR_GREEN: Color = Color::Rgb(154, 205, 50);
const COLOR_GRAY: Color = Color::Rgb(120, 120, 120);
const COLOR_DIM: Color = Color::Rgb(80, 80, 80);
const COLOR_BG_SELECT: Color = Color::Rgb(50, 50, 50);
const COLOR_YELLOW_DIM: Color = Color::Rgb(180, 180, 0);

fn select_skills(items: &mut [ImportItem]) -> Result<bool> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = select_skills_loop(items, &mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn select_skills_loop(
    items: &mut [ImportItem],
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<bool> {
    let mut selected_index: usize = 0;
    let mut scroll_offset: usize = 0;

    loop {
        terminal.draw(|f| {
            render_select_ui(f, items, selected_index, scroll_offset);
        })?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                selected_index = selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if selected_index < items.len() - 1 {
                    selected_index += 1;
                }
            }
            KeyCode::Char(' ') => {
                items[selected_index].selected = !items[selected_index].selected;
            }
            KeyCode::Char('a') => {
                let all_selected = items.iter().all(|item| item.selected);
                let new_state = !all_selected;
                for item in items.iter_mut() {
                    item.selected = new_state;
                }
            }
            KeyCode::Enter => {
                return Ok(true);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                return Ok(false);
            }
            _ => {}
        }

        // スクロール調整（端末の高さは次回描画時に反映されるため概算）
        if selected_index < scroll_offset {
            scroll_offset = selected_index;
        }
    }
}

fn render_select_ui(
    f: &mut Frame,
    items: &[ImportItem],
    selected_index: usize,
    scroll_offset: usize,
) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // ヘッダー
            Constraint::Min(0),    // リスト
            Constraint::Length(2), // フッター
        ])
        .split(area);

    // ヘッダー
    let header = Paragraph::new(Line::from(Span::styled(
        " Select skills to import",
        Style::default()
            .fg(COLOR_YELLOW)
            .add_modifier(Modifier::BOLD),
    )));
    f.render_widget(header, chunks[0]);

    // リスト
    let lines = build_item_lines(items, selected_index);

    let list = Paragraph::new(lines).scroll((scroll_offset as u16, 0));
    f.render_widget(list, chunks[1]);

    // フッター
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" Space", Style::default().fg(COLOR_YELLOW_DIM)),
        Span::styled(":toggle", Style::default().fg(COLOR_DIM)),
        Span::styled("  a", Style::default().fg(COLOR_YELLOW_DIM)),
        Span::styled(":select all", Style::default().fg(COLOR_DIM)),
        Span::styled("  Enter", Style::default().fg(COLOR_YELLOW_DIM)),
        Span::styled(":import", Style::default().fg(COLOR_DIM)),
        Span::styled("  Esc", Style::default().fg(COLOR_YELLOW_DIM)),
        Span::styled(":cancel", Style::default().fg(COLOR_DIM)),
    ]));
    f.render_widget(footer, chunks[2]);
}

fn build_item_lines(items: &[ImportItem], selected_index: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let is_cursor = i == selected_index;

        // カーソル
        let cursor = if is_cursor {
            Span::styled("> ", Style::default().fg(COLOR_YELLOW))
        } else {
            Span::raw("  ")
        };

        // チェックボックス
        let checkbox = if item.selected {
            Span::styled("● ", Style::default().fg(COLOR_GREEN))
        } else {
            Span::styled("  ", Style::default().fg(COLOR_GRAY))
        };

        // スキル名
        let name_style = if is_cursor {
            Style::default()
                .fg(COLOR_YELLOW)
                .bg(COLOR_BG_SELECT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let name = Span::styled(item.name.clone(), name_style);

        // ステータスラベル
        let status = if item.exists_in_store {
            Span::styled(" (already exists)", Style::default().fg(COLOR_GRAY))
        } else {
            Span::styled(" (new)", Style::default().fg(COLOR_GREEN))
        };

        lines.push(Line::from(vec![cursor, checkbox, name, status]));
    }

    lines
}

// ── URLパース ──────────────────────────────────────────────────

fn parse_url(url: &str) -> Result<ParsedRepoUrl> {
    let url = url.trim().trim_end_matches('/');

    let (scheme, path_part) = if let Some(rest) = url.strip_prefix("https://github.com/") {
        ("https://github.com/", rest)
    } else if let Some(rest) = url.strip_prefix("http://github.com/") {
        ("http://github.com/", rest)
    } else {
        anyhow::bail!(
            "Invalid GitHub URL: {}\nExpected format: https://github.com/owner/repo",
            url
        );
    };

    let segments: Vec<&str> = path_part.split('/').collect();
    if segments.len() < 2 || segments[0].is_empty() || segments[1].is_empty() {
        anyhow::bail!(
            "Invalid GitHub URL: {}\nExpected format: https://github.com/owner/repo",
            url
        );
    }

    let owner = segments[0];
    let repo = segments[1].trim_end_matches(".git");

    let subpath = if segments.len() > 3 && segments[2] == "tree" {
        if segments.len() > 4 {
            Some(segments[4..].join("/"))
        } else {
            None
        }
    } else {
        None
    };

    let clone_url = format!("{}{}/{}", scheme, owner, repo);

    Ok(ParsedRepoUrl { clone_url, subpath })
}

// ── git 操作 ───────────────────────────────────────────────────

fn ensure_git_available() -> Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .context("git is not installed or not on PATH")?;

    if !output.status.success() {
        anyhow::bail!("git is not installed or not on PATH");
    }

    Ok(())
}

fn clone_repo(url: &str) -> Result<PathBuf> {
    let tmp_dir =
        std::env::temp_dir().join(format!("skill-switch-man-add-store-{}", std::process::id()));

    if tmp_dir.exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
    }

    let output = Command::new("git")
        .args(["clone", "--depth", "1", url, &tmp_dir.to_string_lossy()])
        .output()
        .context("Failed to execute git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to clone repository:\n{}", stderr.trim());
    }

    Ok(tmp_dir)
}

// ── インポート ─────────────────────────────────────────────────

fn import_skills(skill_dirs: &[PathBuf], store_dir: &Path) -> Result<Vec<ImportAction>> {
    let mut actions = Vec::new();

    for skill_dir in skill_dirs {
        let skill_name = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let target = store_dir.join(&skill_name);

        if target.exists() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("Failed to remove existing skill '{}'", skill_name))?;
            copy_dir_recursive(skill_dir, &target)
                .with_context(|| format!("Failed to copy skill '{}' to store", skill_name))?;
            actions.push(ImportAction::Overwritten {
                name: skill_name.clone(),
            });
        } else {
            copy_dir_recursive(skill_dir, &target)
                .with_context(|| format!("Failed to copy skill '{}' to store", skill_name))?;
            actions.push(ImportAction::Added {
                name: skill_name.clone(),
            });
        }
    }

    Ok(actions)
}

fn prompt_overwrite(skill_name: &str) -> Result<bool> {
    print!(
        "Skill '{}' already exists in the store. Overwrite? [y/N] ",
        skill_name
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let answer = input.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn prompt_subdirectory() -> Result<Option<String>> {
    print!("Import into a subdirectory? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" && input.trim().to_lowercase() != "yes" {
        return Ok(None);
    }

    print!("Subdirectory name: ");
    io::stdout().flush()?;

    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim().to_string();

    if name.is_empty() {
        return Ok(None);
    }

    if name.contains('/') || name.contains('\\') {
        eprintln!("Subdirectory name must not contain path separators. Importing to top level.");
        return Ok(None);
    }

    Ok(Some(name))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

fn print_summary(actions: &[ImportAction]) {
    let mut added = 0;
    let mut overwritten = 0;

    println!();
    println!("Import complete:");
    for action in actions {
        match action {
            ImportAction::Added { name } => {
                println!("  + {}", name);
                added += 1;
            }
            ImportAction::Overwritten { name } => {
                println!("  ~ {} (overwritten)", name);
                overwritten += 1;
            }
        }
    }
    println!();
    println!("{} added, {} overwritten", added, overwritten);
}

// ── テスト ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "skill-switch-man-test-add-store-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_skill(root: &Path, relative_path: &str, description: Option<&str>) -> PathBuf {
        let dir = root.join(relative_path);
        fs::create_dir_all(&dir).unwrap();
        let content = description
            .map(|d| format!("description: {}\n", d))
            .unwrap_or_else(|| "# Skill\n".to_string());
        fs::write(dir.join("SKILL.md"), content).unwrap();
        dir
    }

    // ── parse_url 正常系 ──────────────────────────────────────

    #[test]
    fn parse_url_accepts_standard_https() {
        let parsed = parse_url("https://github.com/owner/repo").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_accepts_git_suffix() {
        let parsed = parse_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_accepts_trailing_slash() {
        let parsed = parse_url("https://github.com/owner/repo/").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_accepts_http_scheme() {
        let parsed = parse_url("http://github.com/owner/repo").unwrap();
        assert_eq!(parsed.clone_url, "http://github.com/owner/repo");
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_accepts_hyphens_dots_numbers() {
        let parsed = parse_url("https://github.com/my-org_v2/skill.pack-3").unwrap();
        assert_eq!(
            parsed.clone_url,
            "https://github.com/my-org_v2/skill.pack-3"
        );
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_extracts_subpath_from_tree_url() {
        let parsed = parse_url(
            "https://github.com/mattpocock/skills/tree/main/skills/productivity/grill-me",
        )
        .unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/mattpocock/skills");
        assert_eq!(
            parsed.subpath.as_deref(),
            Some("skills/productivity/grill-me")
        );
    }

    #[test]
    fn parse_url_tree_without_subpath() {
        let parsed = parse_url("https://github.com/owner/repo/tree/main").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_tree_with_single_subpath() {
        let parsed = parse_url("https://github.com/owner/repo/tree/main/skills").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert_eq!(parsed.subpath.as_deref(), Some("skills"));
    }

    #[test]
    fn parse_url_strips_whitespace() {
        let parsed = parse_url("  https://github.com/owner/repo  ").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert!(parsed.subpath.is_none());
    }

    #[test]
    fn parse_url_git_suffix_with_tree_path() {
        let parsed =
            parse_url("https://github.com/owner/repo.git/tree/main/skills/grill-me").unwrap();
        assert_eq!(parsed.clone_url, "https://github.com/owner/repo");
        assert_eq!(parsed.subpath.as_deref(), Some("skills/grill-me"));
    }

    // ── parse_url 異常系 ──────────────────────────────────────

    #[test]
    fn parse_url_rejects_empty_string() {
        assert!(parse_url("").is_err());
    }

    #[test]
    fn parse_url_rejects_whitespace_only() {
        assert!(parse_url("   ").is_err());
    }

    #[test]
    fn parse_url_rejects_non_github_host() {
        assert!(parse_url("https://gitlab.com/owner/repo").is_err());
        assert!(parse_url("https://bitbucket.org/owner/repo").is_err());
    }

    #[test]
    fn parse_url_rejects_no_scheme() {
        assert!(parse_url("github.com/owner/repo").is_err());
    }

    #[test]
    fn parse_url_rejects_wrong_scheme() {
        assert!(parse_url("ftp://github.com/owner/repo").is_err());
        assert!(parse_url("ssh://github.com/owner/repo").is_err());
    }

    #[test]
    fn parse_url_rejects_ssh_style() {
        assert!(parse_url("git@github.com:owner/repo").is_err());
    }

    #[test]
    fn parse_url_rejects_host_only() {
        assert!(parse_url("https://github.com").is_err());
        assert!(parse_url("https://github.com/").is_err());
    }

    #[test]
    fn parse_url_rejects_owner_only() {
        assert!(parse_url("https://github.com/owner").is_err());
        assert!(parse_url("https://github.com/owner/").is_err());
    }

    #[test]
    fn parse_url_rejects_plain_text() {
        assert!(parse_url("not-a-url").is_err());
        assert!(parse_url("just some words").is_err());
    }

    // ── copy_dir_recursive ───────────────────────────────────────

    #[test]
    fn copy_dir_recursive_copies_files_and_subdirs() {
        let root = temp_root("copy-basic");
        let src = root.join("src");
        let dst = root.join("dst");

        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "hello").unwrap();
        fs::write(src.join("sub/b.txt"), "world").unwrap();
        fs::write(src.join(".hidden"), "skip me").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert!(dst.join("a.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert!(dst.join("sub/b.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "world");
        assert!(!dst.join(".hidden").exists());
    }

    #[test]
    fn copy_dir_recursive_handles_empty_source() {
        let root = temp_root("copy-empty");
        let src = root.join("src");
        let dst = root.join("dst");

        fs::create_dir_all(&src).unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert!(dst.is_dir());
        let entries: Vec<_> = fs::read_dir(&dst).unwrap().collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn copy_dir_recursive_skips_hidden_dirs() {
        let root = temp_root("copy-hidden-dir");
        let src = root.join("src");
        let dst = root.join("dst");

        fs::create_dir_all(src.join(".git")).unwrap();
        fs::create_dir_all(src.join("visible")).unwrap();
        fs::write(src.join(".git/config"), "gitdir").unwrap();
        fs::write(src.join("visible/file.txt"), "data").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert!(!dst.join(".git").exists());
        assert!(dst.join("visible/file.txt").exists());
    }

    #[test]
    fn copy_dir_recursive_handles_deep_nesting() {
        let root = temp_root("copy-deep");
        let src = root.join("src");
        let dst = root.join("dst");

        let deep = src.join("a").join("b").join("c").join("d");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("deep.txt"), "bottom").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(
            fs::read_to_string(dst.join("a/b/c/d/deep.txt")).unwrap(),
            "bottom"
        );
    }

    #[test]
    fn copy_dir_recursive_overwrites_existing_target_files() {
        let root = temp_root("copy-overwrite");
        let src = root.join("src");
        let dst = root.join("dst");

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file.txt"), "new content").unwrap();

        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("file.txt"), "old content").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(
            fs::read_to_string(dst.join("file.txt")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn copy_dir_recursive_errors_on_nonexistent_src() {
        let root = temp_root("copy-noexist");
        let src = root.join("nonexistent");
        let dst = root.join("dst");

        let result = copy_dir_recursive(&src, &dst);
        assert!(result.is_err());
    }

    #[test]
    fn copy_dir_recursive_handles_empty_files() {
        let root = temp_root("copy-empty-file");
        let src = root.join("src");
        let dst = root.join("dst");

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("empty.txt"), "").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert!(dst.join("empty.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("empty.txt")).unwrap(), "");
    }

    // ── scan_skill_dirs ───────────────────────────────────────────

    #[test]
    fn scan_skill_dirs_returns_empty_for_nonexistent_dir() {
        let dirs = crate::skills::scan_skill_dirs(Path::new("/nonexistent-path-xyz")).unwrap();
        assert!(dirs.is_empty());
    }

    #[test]
    fn scan_skill_dirs_finds_top_level_skills() {
        let root = temp_root("scan-top");
        write_skill(&root, "skill-a", Some("A"));
        write_skill(&root, "skill-b", Some("B"));

        let dirs = crate::skills::scan_skill_dirs(&root).unwrap();

        assert_eq!(dirs.len(), 2);
        assert!(dirs.iter().any(|d| d.ends_with("skill-a")));
        assert!(dirs.iter().any(|d| d.ends_with("skill-b")));
    }

    #[test]
    fn scan_skill_dirs_finds_nested_skills() {
        let root = temp_root("scan-nested");
        write_skill(&root, "top-level", None);
        write_skill(&root, "group/nested", None);

        let dirs = crate::skills::scan_skill_dirs(&root).unwrap();

        assert_eq!(dirs.len(), 2);
        assert!(dirs.iter().any(|d| d.ends_with("top-level")));
        assert!(dirs.iter().any(|d| d.ends_with("nested")));
    }

    #[test]
    fn scan_skill_dirs_ignores_hidden_dirs() {
        let root = temp_root("scan-hidden");
        write_skill(&root, "visible", None);
        write_skill(&root, ".hidden-skill", None);

        let dirs = crate::skills::scan_skill_dirs(&root).unwrap();

        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("visible"));
    }

    #[test]
    fn scan_skill_dirs_returns_empty_for_empty_dir() {
        let root = temp_root("scan-empty");
        fs::create_dir_all(&root).unwrap();

        let dirs = crate::skills::scan_skill_dirs(&root).unwrap();

        assert!(dirs.is_empty());
    }

    #[test]
    fn scan_skill_dirs_ignores_dirs_without_skill_md() {
        let root = temp_root("scan-no-md");
        let plain_dir = root.join("not-a-skill");
        fs::create_dir_all(&plain_dir).unwrap();
        fs::write(plain_dir.join("README.md"), "readme").unwrap();

        let dirs = crate::skills::scan_skill_dirs(&root).unwrap();

        assert!(dirs.is_empty());
    }

    #[test]
    fn scan_skill_dirs_deeply_nested() {
        let root = temp_root("scan-deep");
        write_skill(&root, "a/b/c/deep-skill", None);

        let dirs = crate::skills::scan_skill_dirs(&root).unwrap();

        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("deep-skill"));
    }

    // ── import_skills ─────────────────────────────────────────────

    #[test]
    fn import_skills_adds_new_skills() {
        let root = temp_root("import-new");
        let store = root.join("store");
        let repo = root.join("repo");

        write_skill(&repo, "skill-a", None);
        write_skill(&repo, "skill-b", None);

        let skill_dirs = crate::skills::scan_skill_dirs(&repo).unwrap();
        let actions = import_skills(&skill_dirs, &store).unwrap();

        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], ImportAction::Added { name } if name == "skill-a"));
        assert!(matches!(&actions[1], ImportAction::Added { name } if name == "skill-b"));
        assert!(store.join("skill-a/SKILL.md").exists());
        assert!(store.join("skill-b/SKILL.md").exists());
    }

    #[test]
    fn import_skills_overwrites_existing() {
        let root = temp_root("import-overwrite");
        let store = root.join("store");
        let repo = root.join("repo");

        fs::create_dir_all(store.join("my-skill")).unwrap();
        fs::write(store.join("my-skill/SKILL.md"), "old").unwrap();

        write_skill(&repo, "my-skill", Some("new"));

        let skill_dirs = crate::skills::scan_skill_dirs(&repo).unwrap();
        let actions = import_skills(&skill_dirs, &store).unwrap();

        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ImportAction::Overwritten { name } if name == "my-skill"));
        let content = fs::read_to_string(store.join("my-skill/SKILL.md")).unwrap();
        assert!(content.contains("new"));
    }

    #[test]
    fn import_skills_mixed_add_and_overwrite() {
        let root = temp_root("import-mixed");
        let store = root.join("store");
        let repo = root.join("repo");

        fs::create_dir_all(store.join("existing")).unwrap();
        fs::write(store.join("existing/SKILL.md"), "old").unwrap();

        write_skill(&repo, "new-skill", None);
        write_skill(&repo, "existing", Some("updated"));

        let skill_dirs = crate::skills::scan_skill_dirs(&repo).unwrap();
        let actions = import_skills(&skill_dirs, &store).unwrap();

        assert_eq!(actions.len(), 2);

        let added: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                ImportAction::Added { name } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert!(added.contains(&"new-skill".to_string()));

        let overwritten: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                ImportAction::Overwritten { name } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert!(overwritten.contains(&"existing".to_string()));
        assert_eq!(
            fs::read_to_string(store.join("existing/SKILL.md")).unwrap(),
            "description: updated\n"
        );
    }

    #[test]
    fn import_skills_empty_list() {
        let root = temp_root("import-empty");
        let store = root.join("store");
        fs::create_dir_all(&store).unwrap();

        let actions = import_skills(&[], &store).unwrap();

        assert!(actions.is_empty());
    }

    #[test]
    fn import_skills_preserves_subdirectory_contents() {
        let root = temp_root("import-subdirs");
        let store = root.join("store");
        let repo = root.join("repo");

        let skill_dir = repo.join("complex-skill");
        fs::create_dir_all(skill_dir.join("templates")).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "description: complex\n").unwrap();
        fs::write(skill_dir.join("templates/tmpl.txt"), "template data").unwrap();
        fs::write(skill_dir.join("config.json"), "{\"key\":\"val\"}").unwrap();

        let skill_dirs = crate::skills::scan_skill_dirs(&repo).unwrap();
        import_skills(&skill_dirs, &store).unwrap();

        assert!(store.join("complex-skill/SKILL.md").exists());
        assert!(store.join("complex-skill/templates/tmpl.txt").exists());
        assert!(store.join("complex-skill/config.json").exists());
        assert_eq!(
            fs::read_to_string(store.join("complex-skill/templates/tmpl.txt")).unwrap(),
            "template data"
        );
    }

    // ── build_import_items ────────────────────────────────────────

    #[test]
    fn build_items_new_skills_default_selected() {
        let root = temp_root("items-new");
        let store = root.join("store");
        let repo = root.join("repo");

        write_skill(&repo, "skill-a", None);
        write_skill(&repo, "skill-b", None);

        let dirs = crate::skills::scan_skill_dirs(&repo).unwrap();
        let items = build_import_items(&dirs, &store);

        assert_eq!(items.len(), 2);
        assert!(items[0].selected); // 新規 → デフォルト選択
        assert!(items[1].selected);
        assert!(!items[0].exists_in_store);
        assert!(!items[1].exists_in_store);
    }

    #[test]
    fn build_items_existing_skills_default_unselected() {
        let root = temp_root("items-existing");
        let store = root.join("store");
        let repo = root.join("repo");

        // ストアに事前に用意
        fs::create_dir_all(store.join("old-skill")).unwrap();
        fs::write(store.join("old-skill/SKILL.md"), "old").unwrap();

        write_skill(&repo, "old-skill", None);
        write_skill(&repo, "new-skill", None);

        let dirs = crate::skills::scan_skill_dirs(&repo).unwrap();
        let items = build_import_items(&dirs, &store);

        let old = items.iter().find(|i| i.name == "old-skill").unwrap();
        assert!(!old.selected); // 既存 → デフォルト未選択
        assert!(old.exists_in_store);

        let new = items.iter().find(|i| i.name == "new-skill").unwrap();
        assert!(new.selected); // 新規 → デフォルト選択
        assert!(!new.exists_in_store);
    }

    // ── print_summary ─────────────────────────────────────────────

    #[test]
    fn print_summary_does_not_panic_on_empty() {
        print_summary(&[]);
    }

    #[test]
    fn print_summary_does_not_panic_on_mixed_actions() {
        let actions = vec![
            ImportAction::Added {
                name: "a".to_string(),
            },
            ImportAction::Overwritten {
                name: "b".to_string(),
            },
        ];
        print_summary(&actions);
    }

    // ── ensure_git_available ───────────────────────────────────────

    #[test]
    fn ensure_git_available_succeeds_when_git_is_installed() {
        let result = ensure_git_available();
        assert!(result.is_ok());
    }
}
