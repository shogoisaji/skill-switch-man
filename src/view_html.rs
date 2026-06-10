use crate::config::{collapse_tilde, Agent, Config};
use crate::skills::{list_skills, list_untracked_skills, Skill, SkillNode, UntrackedSkill};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn run() -> Result<()> {
    let config = Config::load()?;
    let source_dir = config.get_skills_source_dir();
    let skills = list_skills(&source_dir)?;

    let mut untracked_by_agent = Vec::new();
    for agent in Agent::ALL {
        let untracked = list_untracked_skills(&config, &skills, agent)?;
        untracked_by_agent.push((agent, untracked));
    }

    let all_skills = collect_all_skills(&skills);
    let html = generate_html(&config, &all_skills, &untracked_by_agent);

    let path = std::env::temp_dir().join("skill-switch-man-status.html");
    fs::write(&path, &html)
        .with_context(|| format!("Failed to write HTML to {}", path.display()))?;

    println!("Status page written to: {}", path.display());

    match open_in_browser(&path) {
        Ok(()) => {
            // Give the browser a moment to read the file before deleting it
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = fs::remove_file(&path);
        }
        Err(e) => {
            eprintln!("Could not open browser: {e}");
            eprintln!("Open manually: {}", path.display());
        }
    }

    Ok(())
}

fn collect_all_skills(nodes: &[SkillNode]) -> Vec<&Skill> {
    let mut result = Vec::new();
    collect_all_skills_recursive(nodes, &mut result);
    result
}

fn collect_all_skills_recursive<'a>(nodes: &'a [SkillNode], out: &mut Vec<&'a Skill>) {
    for node in nodes {
        match node {
            SkillNode::Skill(skill) => out.push(skill),
            SkillNode::Folder { children, .. } => collect_all_skills_recursive(children, out),
        }
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn generate_html(
    config: &Config,
    skills: &[&Skill],
    untracked_by_agent: &[(Agent, Vec<UntrackedSkill>)],
) -> String {
    let source_dir_display = collapse_tilde(&config.get_skills_source_dir());

    let mut agent_sections = String::new();
    let mut untracked_sections = String::new();

    for agent in Agent::ALL {
        let (r, g, b) = agent.accent_rgb();
        let color = format!("rgb({r}, {g}, {b})");
        let target_dir = agent
            .target_dir()
            .map(|p| collapse_tilde(&p))
            .unwrap_or_else(|_| "—".to_string());
        let enabled: Vec<_> = skills
            .iter()
            .filter(|s| config.is_skill_enabled(agent, &s.relative_path))
            .collect();
        let enabled_count = enabled.len();

        agent_sections.push_str(&format!(
            r#"            <div class="agent-card" style="border-left: 4px solid {color}">
                <h3 style="color: {color}">{name}</h3>
                <div class="agent-info">
                    <div class="info-row">
                        <span class="label">Target:</span>
                        <code>{target}</code>
                    </div>
                    <div class="info-row">
                        <span class="label">Enabled:</span>
                        <span class="badge" style="background: {color}">{count} skill{plural}</span>
                    </div>
                </div>
            </div>
"#,
            name = html_escape(agent.display_name()),
            target = html_escape(&target_dir),
            count = enabled_count,
            plural = if enabled_count == 1 { "" } else { "s" },
            color = color,
        ));

        let (_, untracked) = untracked_by_agent
            .iter()
            .find(|(a, _)| *a == agent)
            .unwrap();

        if !untracked.is_empty() {
            let mut rows = String::new();
            for skill in untracked {
                let conflict_icon = if skill.conflicts_with_source {
                    r#"<span class="conflict">⚠ conflict</span>"#
                } else {
                    ""
                };
                let link_info = skill
                    .link_target
                    .as_ref()
                    .map(|t| format!("<br><small>→ {}</small>", html_escape(&t.to_string_lossy())))
                    .unwrap_or_default();
                rows.push_str(&format!(
                    r#"                        <tr>
                            <td>{name}</td>
                            <td><code>{path}</code>{link}</td>
                            <td>{conflict}</td>
                        </tr>
"#,
                    name = html_escape(&skill.name),
                    path = html_escape(&skill.path.to_string_lossy()),
                    link = link_info,
                    conflict = conflict_icon,
                ));
            }

            untracked_sections.push_str(&format!(
                r#"            <div class="untracked-group">
                <h3 style="color: {color}">{name}</h3>
                <table>
                    <thead><tr><th>Name</th><th>Path</th><th>Status</th></tr></thead>
                    <tbody>
{rows}                    </tbody>
                </table>
            </div>
"#,
                name = html_escape(agent.display_name()),
                color = color,
                rows = rows,
            ));
        }
    }

    let mut skill_rows = String::new();
    for skill in skills {
        let description = skill
            .description
            .as_deref()
            .map(html_escape)
            .unwrap_or_else(|| "<span class=\"muted\">No description</span>".to_string());

        let mut status_cells = String::new();
        for agent in Agent::ALL {
            let (r, g, b) = agent.accent_rgb();
            let enabled = config.is_skill_enabled(agent, &skill.relative_path);
            if enabled {
                status_cells.push_str(&format!(
                    r#"<td class="enabled" style="color: rgb({r},{g},{b})">✓</td>"#
                ));
            } else {
                status_cells.push_str(r#"<td class="disabled">—</td>"#);
            }
        }

        skill_rows.push_str(&format!(
            r#"                    <tr>
                        <td><strong>{name}</strong></td>
                        <td><code>{path}</code></td>
                        <td class="desc-cell" onclick="this.classList.toggle('expanded')" title="Click to expand">{desc}</td>
                        {cells}
                    </tr>
"#,
            name = html_escape(&skill.name),
            path = html_escape(&skill.relative_path),
            desc = description,
            cells = status_cells,
        ));
    }

    if skills.is_empty() {
        skill_rows.push_str(
            r#"                    <tr><td colspan="6" class="muted">No skills found</td></tr>
"#,
        );
    }

    let untracked_section = if untracked_sections.is_empty() {
        r#"        <section id="untracked">
            <h2>Untracked Skills</h2>
            <p class="muted">No untracked skills found.</p>
        </section>
"#
        .to_string()
    } else {
        format!(
            r#"        <section id="untracked">
            <h2>Untracked Skills</h2>
{untracked_sections}        </section>
"#
        )
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Skill Switch Man — Status</title>
    <style>
        :root {{
            --bg: #1a1a2e;
            --surface: #16213e;
            --surface2: #1f2b47;
            --border: #2a3a5c;
            --text: #e0e0e0;
            --muted: #6b7b9e;
            --claude: rgb(255,165,0);
            --codex: rgb(154,205,50);
            --opencode: rgb(180,120,255);
        }}
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
            background: var(--bg);
            color: var(--text);
            line-height: 1.6;
            padding: 2rem;
            max-width: 1100px;
            margin: 0 auto;
        }}
        h1 {{
            font-size: 1.8rem;
            margin-bottom: 0.25rem;
        }}
        h2 {{
            font-size: 1.3rem;
            margin-bottom: 1rem;
            padding-bottom: 0.5rem;
            border-bottom: 1px solid var(--border);
        }}
        h3 {{
            font-size: 1.05rem;
            margin-bottom: 0.5rem;
        }}
        .subtitle {{
            color: var(--muted);
            font-size: 0.9rem;
            margin-bottom: 2rem;
        }}
        section {{
            margin-bottom: 2.5rem;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            font-size: 0.9rem;
        }}
        th, td {{
            padding: 0.55rem 0.75rem;
            text-align: left;
            border-bottom: 1px solid var(--border);
        }}
        th {{
            color: var(--muted);
            font-weight: 500;
            text-transform: uppercase;
            font-size: 0.8rem;
            letter-spacing: 0.05em;
        }}
        tr:hover {{ background: var(--surface2); }}
        code {{
            background: var(--surface2);
            padding: 0.15rem 0.4rem;
            border-radius: 3px;
            font-size: 0.85rem;
        }}
        strong {{ font-weight: 600; }}
        .muted {{ color: var(--muted); }}
        .badge {{
            display: inline-block;
            padding: 0.15rem 0.6rem;
            border-radius: 999px;
            font-size: 0.8rem;
            color: var(--bg);
            font-weight: 600;
        }}
        .info-row {{
            display: flex;
            gap: 0.5rem;
            align-items: center;
            margin-bottom: 0.25rem;
        }}
        .label {{
            color: var(--muted);
            min-width: 60px;
        }}
        .agent-cards {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 1rem;
        }}
        .agent-card {{
            background: var(--surface);
            padding: 1rem 1.25rem;
            border-radius: 8px;
        }}
        td.enabled {{ font-weight: 700; text-align: center; font-size: 1.1rem; }}
        td.disabled {{ color: var(--muted); text-align: center; font-size: 1.1rem; }}
        .desc-cell {{
            max-width: 300px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            cursor: pointer;
            user-select: none;
            transition: all 0.15s ease;
        }}
        .desc-cell:hover {{ color: #fff; }}
        .desc-cell.expanded {{
            white-space: normal;
            overflow: visible;
        }}
        .conflict {{
            color: #ff6b6b;
            font-size: 0.85rem;
        }}
        .untracked-group {{
            margin-bottom: 1.25rem;
        }}
        .table-wrap {{
            overflow-x: auto;
        }}
        .store-info {{
            background: var(--surface);
            padding: 1rem 1.25rem;
            border-radius: 8px;
            margin-bottom: 0.5rem;
        }}
    </style>
</head>
<body>
    <h1>⚡ Skill Switch Man</h1>
    <p class="subtitle">Generated at {timestamp}</p>

    <section id="store">
        <h2>Skill Store</h2>
        <div class="store-info">
            <div class="info-row">
                <span class="label">Source:</span>
                <code>{source_dir}</code>
            </div>
            <div class="info-row">
                <span class="label">Skills:</span>
                <span>{count} skill{plural}</span>
            </div>
        </div>
    </section>

    <section id="agents">
        <h2>Agents</h2>
        <div class="agent-cards">
{agent_sections}        </div>
    </section>

    <section id="skills">
        <h2>Skill Matrix</h2>
        <div class="table-wrap">
            <table>
                <thead>
                    <tr>
                        <th>Name</th>
                        <th>Path</th>
                        <th>Description</th>
                        <th style="color:var(--claude)">Claude</th>
                        <th style="color:var(--codex)">Codex</th>
                        <th style="color:var(--opencode)">OpenCode</th>
                    </tr>
                </thead>
                <tbody>
{skill_rows}                </tbody>
            </table>
        </div>
    </section>

{untracked_section}</body>
</html>"##,
        timestamp = html_escape(&format_timestamp()),
        source_dir = html_escape(&source_dir_display),
        count = skills.len(),
        plural = if skills.len() == 1 { "" } else { "s" },
        agent_sections = agent_sections,
        skill_rows = skill_rows,
        untracked_section = untracked_section,
    )
}

fn format_timestamp() -> String {
    let total_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Howard Hinnant's civil_from_days algorithm
    let days = (total_secs / 86400) as i64;
    let time_of_day = (total_secs % 86400) as u64;

    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 4 + doe / 100 - doe / 400) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100 + yoe / 400);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let h = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        y, m, d, h, min, s
    )
}

fn open_in_browser(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&absolute)
            .status()
            .with_context(|| "Failed to run `open` command")?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&absolute)
            .status()
            .with_context(|| "Failed to run `xdg-open` command")?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", &absolute.to_string_lossy()])
            .status()
            .with_context(|| "Failed to run `start` command")?;
    }

    Ok(())
}
