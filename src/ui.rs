use crate::app::{App, CurrentScreen, VisibleItem};
use crate::config::Agent;
use crate::skills::{SkillNode, UntrackedSkill};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

const C_YELLOW: Color = Color::Rgb(255, 255, 0);
const C_GREEN: Color = Color::Rgb(154, 205, 50);
const C_SELECTED_BG: Color = Color::Rgb(50, 50, 50);

pub fn ui(f: &mut Frame, app: &mut App) {
    match app.current_screen {
        CurrentScreen::Settings | CurrentScreen::EditingSkillsSourcePath => {
            render_settings_screen(f, app, f.area())
        }
        CurrentScreen::Help => render_help_screen(f, f.area()),
        _ => render_home_screen(f, app, f.area()),
    }

    if app.current_screen == CurrentScreen::Confirmation {
        render_confirmation_dialog(f, app, f.area());
    }
}

fn render_home_screen(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = agent_color(app.active_agent);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(6),
        ])
        .split(area);

    let tabs = Tabs::new(
        Agent::ALL
            .iter()
            .map(|agent| Line::from(agent.name()))
            .collect::<Vec<_>>(),
    )
    .select(app.active_agent.index())
    .highlight_style(Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD))
    .divider(Span::raw(" | "))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Target tool")
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(accent)),
    );
    f.render_widget(tabs, chunks[0]);

    if chunks[1].width >= 90 {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(64), Constraint::Min(30)])
            .split(chunks[1]);
        render_skill_list(f, app, body[0]);
        render_details(f, app, body[1]);
    } else if chunks[1].height >= 14 {
        let detail_height =
            detail_panel_height(app, chunks[1].width).min(chunks[1].height.saturating_sub(4));
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(detail_height)])
            .split(chunks[1]);
        render_skill_list(f, app, body[0]);
        render_details(f, app, body[1]);
    } else {
        render_skill_list(f, app, chunks[1]);
    }

    let guide = Paragraph::new(home_footer_lines(app))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Quick guide")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        );
    f.render_widget(guide, chunks[2]);
}

fn render_skill_list(f: &mut Frame, app: &mut App, area: Rect) {
    let list_view_height = area.height.saturating_sub(2) as usize;
    app.ensure_selection_visible(list_view_height);

    let lines = build_skill_lines(app);
    let content = if lines.is_empty() {
        vec![Line::from(Span::styled(
            "No skills found in the shared store.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        lines
    };

    let list = Paragraph::new(content)
        .scroll((app.list_scroll_offset as u16, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    "{} - managed skill tree",
                    app.active_agent.display_name()
                ))
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(agent_color(app.active_agent))),
        );
    f.render_widget(list, area);
}

fn build_skill_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut visible_index = 0;
    append_skill_lines(app, &app.skills, &[], &mut visible_index, &mut lines);

    for (index, skill) in app.active_untracked_skills().iter().enumerate() {
        lines.push(build_untracked_line(
            skill,
            visible_index + index == app.selected_index,
        ));
    }

    lines
}

fn append_skill_lines(
    app: &App,
    nodes: &[SkillNode],
    ancestor_has_next_sibling: &[bool],
    visible_index: &mut usize,
    lines: &mut Vec<Line<'static>>,
) {
    for (node_index, node) in nodes.iter().enumerate() {
        let is_last = node_index + 1 == nodes.len();
        let prefix = tree_prefix(ancestor_has_next_sibling, is_last);
        lines.push(build_skill_node_line(app, node, *visible_index, prefix));
        *visible_index += 1;

        if let SkillNode::Folder {
            expanded: true,
            children,
            ..
        } = node
        {
            let mut child_ancestors = ancestor_has_next_sibling.to_vec();
            child_ancestors.push(!is_last);
            append_skill_lines(app, children, &child_ancestors, visible_index, lines);
        }
    }
}

fn tree_prefix(ancestor_has_next_sibling: &[bool], is_last: bool) -> String {
    if ancestor_has_next_sibling.is_empty() {
        return String::new();
    }

    let mut prefix = String::new();
    for has_next_sibling in ancestor_has_next_sibling {
        prefix.push_str(if *has_next_sibling { "│  " } else { "   " });
    }
    prefix.push_str(if is_last { "└─ " } else { "├─ " });
    prefix
}

fn build_skill_node_line(
    app: &App,
    node: &SkillNode,
    index: usize,
    tree_prefix: String,
) -> Line<'static> {
    let is_selected = index == app.selected_index;
    let cursor = if is_selected { ">" } else { " " };

    let base_style = if is_selected {
        Style::default().bg(C_SELECTED_BG).fg(C_YELLOW)
    } else {
        Style::default().fg(Color::White)
    };

    let (state_label, state_style) = if let Some(skill) = node.skill() {
        let enabled = app
            .config
            .is_skill_enabled(app.active_agent, &skill.relative_path);
        let conflicts_with_untracked = app.has_untracked_conflict(app.active_agent, &skill.name);

        if enabled {
            ("ON", status_style(C_GREEN, is_selected))
        } else if conflicts_with_untracked {
            ("BLOCKED", status_style(Color::Red, is_selected))
        } else {
            ("OFF", status_style(Color::Red, is_selected))
        }
    } else {
        ("FOLDER", status_style(Color::Gray, is_selected))
    };

    let mut spans = vec![
        Span::styled(format!("{} ", cursor), base_style),
        Span::styled(format!("{:<12}", format!("[{}]", state_label)), state_style),
        Span::styled(tree_prefix, base_style),
    ];

    if node.is_folder() {
        let fold_marker = if node.is_expanded() { "▼" } else { "▶" };
        spans.push(Span::styled(format!("{} ", fold_marker), base_style));
    }

    let name = if node.is_folder() {
        format!("{}/", node.name())
    } else {
        node.name().to_string()
    };
    spans.push(Span::styled(name, base_style.add_modifier(Modifier::BOLD)));

    Line::from(spans)
}

fn build_untracked_line(skill: &UntrackedSkill, is_selected: bool) -> Line<'static> {
    let cursor = if is_selected { ">" } else { " " };
    let base_style = if is_selected {
        Style::default().bg(C_SELECTED_BG).fg(C_YELLOW)
    } else {
        Style::default().fg(Color::White)
    };
    let state_style = status_style(Color::Red, is_selected);
    let conflict = if skill.conflicts_with_source {
        " [CONFLICT]"
    } else {
        ""
    };

    Line::from(vec![
        Span::styled(format!("{} ", cursor), base_style),
        Span::styled(format!("{:<12}", "[UNTRACKED]"), state_style),
        Span::styled(skill.name.clone(), base_style.add_modifier(Modifier::BOLD)),
        Span::styled(conflict.to_string(), Style::default().fg(Color::Red)),
    ])
}

fn render_details(f: &mut Frame, app: &App, area: Rect) {
    let content = Paragraph::new(detail_lines(app))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Selected item")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(agent_color(app.active_agent))),
        );
    f.render_widget(content, area);
}

fn detail_panel_height(app: &App, width: u16) -> u16 {
    let content_width = width.saturating_sub(2);
    let paragraph = Paragraph::new(detail_lines(app))
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL));
    paragraph.line_count(content_width).min(u16::MAX as usize) as u16
}

fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(item) = app.visible_items().get(app.selected_index).copied() else {
        return vec![Line::from(Span::styled(
            "Select a skill or folder to see details.",
            Style::default().fg(Color::DarkGray),
        ))];
    };

    match item {
        VisibleItem::SkillNode(node) => skill_detail_lines(app, node),
        VisibleItem::UntrackedSkill(skill) => untracked_detail_lines(skill),
    }
}

fn skill_detail_lines(app: &App, node: &SkillNode) -> Vec<Line<'static>> {
    if node.is_folder() {
        return folder_detail_lines(node);
    }

    let Some(skill) = node.skill() else {
        return Vec::new();
    };

    let enabled = app
        .config
        .is_skill_enabled(app.active_agent, &skill.relative_path);
    let saved_enabled = app
        .saved_config
        .is_skill_enabled(app.active_agent, &skill.relative_path);
    let conflict = app.has_untracked_conflict(app.active_agent, &skill.name);
    let status = if conflict && !enabled {
        ("BLOCKED", Color::Red)
    } else if enabled {
        ("ON", C_GREEN)
    } else {
        ("OFF", Color::Red)
    };

    let description = skill
        .description
        .clone()
        .unwrap_or_else(|| "No description in SKILL.md.".to_string());

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Skill  ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                skill.name.clone(),
                Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
            ),
        ]),
        section_line_with_color("Description", Color::Cyan),
        Line::from(Span::styled(description, Style::default().fg(Color::White))),
        Line::from(""),
        key_value("Type", "Skill"),
        key_value("Path", skill.relative_path.clone()),
        key_value_with_style("Status", status.0, status.1),
        key_value(
            "Target",
            format!(
                "~/{}/skills/{}",
                app.active_agent.home_dir_name(),
                skill.name
            ),
        ),
    ];

    if enabled != saved_enabled {
        lines.push(key_value("Changes", "pending (not saved)"));
    }

    lines
}

fn folder_detail_lines(node: &SkillNode) -> Vec<Line<'static>> {
    let state = if node.is_expanded() {
        ("EXPANDED", C_GREEN)
    } else {
        ("COLLAPSED", Color::Gray)
    };

    vec![
        Line::from(Span::styled(
            format!("{}/", node.name()),
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )),
        key_value("Type", "Folder"),
        key_value("Path", node.relative_path().to_string()),
        key_value_with_style("State", state.0, state.1),
        key_value("Contains", format!("{} skill(s)", count_skills(node))),
    ]
}

fn untracked_detail_lines(skill: &UntrackedSkill) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            skill.name.clone(),
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )),
        key_value("Type", "Untracked entry"),
        key_value("Path", skill.path.display().to_string()),
        key_value_with_style("Status", "UNTRACKED", Color::Red),
    ];

    if let Some(target) = &skill.link_target {
        lines.push(key_value("Link target", target.display().to_string()));
    }

    lines.extend([
        Line::from(""),
        section_line("Why is this listed?"),
        Line::from(
            "It exists in the selected tool's skills directory, but it is not linked from the shared store.",
        ),
    ]);

    if skill.conflicts_with_source {
        lines.push(key_value_with_style(
            "Warning",
            "same name exists in the shared store",
            Color::Red,
        ));
        lines.push(action_line(
            "Rename or remove this entry before enabling the shared skill.",
        ));
    } else {
        lines.push(action_line(
            "This entry is read-only here; manage it outside skill-switch-man.",
        ));
    }

    lines
}

fn count_skills(node: &SkillNode) -> usize {
    match node {
        SkillNode::Skill(_) => 1,
        SkillNode::Folder { children, .. } => children.iter().map(count_skills).sum(),
    }
}

fn key_value(label: &str, value: impl Into<String>) -> Line<'static> {
    key_value_with_style(label, value, Color::White)
}

fn key_value_with_style(
    label: &str,
    value: impl Into<String>,
    value_color: Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}: ", label),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.into(), Style::default().fg(value_color)),
    ])
}

fn section_line(title: &str) -> Line<'static> {
    section_line_with_color(title, C_YELLOW)
}

fn section_line_with_color(title: &str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn action_line(text: &str) -> Line<'static> {
    Line::from(vec![Span::raw("  "), Span::raw(text.to_string())])
}

fn status_style(color: Color, is_selected: bool) -> Style {
    let mut style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    if is_selected {
        style = style.bg(C_SELECTED_BG);
    }
    style
}

fn render_settings_screen(f: &mut Frame, app: &App, area: Rect) {
    let accent = agent_color(app.active_agent);
    let editing = app.current_screen == CurrentScreen::EditingSkillsSourcePath;
    let value = if editing {
        format!("{}|", app.input_buffer)
    } else {
        app.config.skills_source_dir.clone()
    };

    let content = Paragraph::new(settings_lines(value, editing))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if editing {
                    "Edit skill store path"
                } else {
                    "Settings"
                })
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        );
    f.render_widget(content, area);
}

fn settings_lines(value: String, editing: bool) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "Shared skill store",
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        key_value("Path", value),
        Line::from(""),
        Line::from("This directory is the source of truth for reusable skills."),
        Line::from("Enabled skills are linked into each selected tool's directory."),
        Line::from(""),
        section_line("Target directories"),
    ];

    lines.extend(Agent::ALL.into_iter().map(|agent| {
        key_value(
            agent.display_name(),
            format!("~/{}/skills", agent.home_dir_name()),
        )
    }));

    lines.extend([
        Line::from(""),
        section_line("Controls"),
        Line::from(if editing {
            "Type a path, then press Enter to save or Esc to cancel."
        } else {
            "Enter/Space edits the path. Esc returns to the skill list."
        }),
        Line::from("Press ? or F1 for help. Press q to quit."),
    ]);

    lines
}

fn home_footer_lines(app: &App) -> Vec<Line<'static>> {
    let message = app
        .message
        .as_ref()
        .map(|message| format!("Message: {}", message))
        .unwrap_or_else(|| "Select an item to see its details.".to_string());

    let key_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let status_style = |color| Style::default().fg(color).add_modifier(Modifier::BOLD);

    vec![
        Line::from(Span::styled(
            message,
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("←→", key_style),
            Span::raw(" Tool   "),
            Span::styled("↑↓", key_style),
            Span::raw(" Select   "),
            Span::styled("Space", key_style),
            Span::raw(" Toggle/expand   "),
            Span::styled("Enter", key_style),
            Span::raw(" Review/apply"),
        ]),
        Line::from(vec![
            Span::styled("s", key_style),
            Span::raw(" Settings   "),
            Span::styled("?", key_style),
            Span::raw(" Help   "),
            Span::styled("q", key_style),
            Span::raw(" Quit"),
        ]),
        Line::from(vec![
            Span::styled("ON", status_style(C_GREEN)),
            Span::raw(" active   "),
            Span::styled("OFF", status_style(Color::Red)),
            Span::raw(" idle   "),
            Span::styled("BLOCKED", status_style(Color::Red)),
            Span::raw(" conflict   "),
            Span::styled("UNTRACKED", status_style(Color::Red)),
            Span::raw(" outside   "),
            Span::styled("FOLDER", status_style(Color::Gray)),
            Span::raw(" group"),
        ]),
    ]
}

fn render_help_screen(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Skill Switch Man",
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )),
        Line::from("The list manages skills from one shared store for the selected tool."),
        Line::from("A skill is enabled by creating a link in that tool's skills directory."),
        Line::from(""),
        section_line("Status labels"),
        help_row("[ON]", "Enabled for the selected tool."),
        help_row("[OFF]", "Available in the store, but not enabled."),
        help_row(
            "[BLOCKED]",
            "Cannot enable because an untracked entry has the same name.",
        ),
        help_row(
            "[UNTRACKED]",
            "Already present in the tool directory, outside the shared store.",
        ),
        help_row(
            "[FOLDER]",
            "A group; folders are expanded/collapsed, not enabled.",
        ),
        help_row(
            "│ ├─ └─",
            "Tree guides show parent, child, and sibling relationships.",
        ),
        Line::from(""),
        section_line("Key bindings"),
        help_row("←/→, h/l", "Switch the selected target tool."),
        help_row("↑/↓, j/k", "Move through skills and folders."),
        help_row("Space", "Toggle a skill, or expand/collapse a folder."),
        help_row("Enter", "Review the pending changes, then apply or cancel."),
        help_row("s", "Open settings and change the shared store path."),
        help_row("Esc", "Quit from the list; go back from settings/help."),
        help_row("q", "Quit the application."),
        Line::from(""),
        Line::from("Press Esc, ? or F1 to return."),
    ];

    let content = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Help")
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(content, area);
}

fn help_row(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<18}", key),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(description.to_string()),
    ])
}

fn render_confirmation_dialog(f: &mut Frame, app: &App, area: Rect) {
    let accent = agent_color(app.active_agent);
    let popup = centered_rect(area, 78, 20);
    let popup_style = Style::default().bg(Color::Rgb(18, 18, 18)).fg(Color::White);
    let apply_style = if app.confirm_apply_yes {
        Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let cancel_style = if app.confirm_apply_yes {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "Review pending changes",
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )),
        Line::from("These links and settings will change only if you apply them."),
        Line::from(""),
    ];

    let changes = app.pending_changes();
    if changes.iter().all(|change| change.is_empty()) {
        lines.push(Line::from(Span::styled(
            "No pending changes.",
            Style::default().fg(Color::Gray),
        )));
    } else {
        for change in changes {
            if change.is_empty() {
                continue;
            }

            lines.push(Line::from(Span::styled(
                change.agent.display_name(),
                Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
            )));

            for skill in change.added {
                lines.push(Line::from(vec![
                    Span::styled("+ Enable  ", Style::default().fg(C_GREEN)),
                    Span::raw(skill.relative_path),
                ]));
            }

            for skill in change.removed {
                lines.push(Line::from(vec![
                    Span::styled("- Disable ", Style::default().fg(Color::Red)),
                    Span::raw(skill.relative_path),
                ]));
            }

            lines.push(Line::from(""));
        }
    }

    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::raw("["),
            Span::styled("Apply", apply_style),
            Span::raw("]   ["),
            Span::styled("Cancel", cancel_style),
            Span::raw("]"),
        ]),
        Line::from("←/→ or h/l selects   y/n selects   Enter confirms   Esc backs out"),
    ]);

    let content = Paragraph::new(lines)
        .style(popup_style)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .style(popup_style)
                .borders(Borders::ALL)
                .title("Apply changes?")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        );
    f.render_widget(Clear, popup);
    f.render_widget(content, popup);
}

fn agent_color(agent: Agent) -> Color {
    let (red, green, blue) = agent.accent_rgb();
    Color::Rgb(red, green, blue)
}

fn centered_rect(area: Rect, width_percent: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

#[cfg(test)]
mod tests {
    use super::ui;
    use crate::app::{App, CurrentScreen};
    use crate::config::{Agent, Config};
    use crate::skills::{Skill, SkillNode};
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    fn app_with_skill() -> App {
        App {
            config: Config::default(),
            saved_config: Config::default(),
            skills: vec![SkillNode::Skill(Skill {
                name: "writer".to_string(),
                path: PathBuf::from("/tmp/skill-store/writer"),
                relative_path: "writer".to_string(),
                description: Some("Writes clear technical documentation.".to_string()),
            })],
            untracked_skills: Vec::new(),
            selected_index: 0,
            list_scroll_offset: 0,
            active_agent: Agent::Claude,
            message: None,
            current_screen: CurrentScreen::Home,
            input_buffer: String::new(),
            confirm_apply_yes: true,
            help_return_screen: CurrentScreen::Home,
        }
    }

    fn app_with_nested_skills() -> App {
        let mut app = app_with_skill();
        app.skills = vec![
            SkillNode::Skill(Skill {
                name: "root-skill".to_string(),
                path: PathBuf::from("/tmp/skill-store/root-skill"),
                relative_path: "root-skill".to_string(),
                description: Some("A skill at the root of the store.".to_string()),
            }),
            SkillNode::Folder {
                name: "debug-samples".to_string(),
                relative_path: "debug-samples".to_string(),
                expanded: true,
                children: vec![
                    SkillNode::Folder {
                        name: "diagnostics".to_string(),
                        relative_path: "debug-samples/diagnostics".to_string(),
                        expanded: true,
                        children: vec![
                            SkillNode::Skill(Skill {
                                name: "log-analysis".to_string(),
                                path: PathBuf::from(
                                    "/tmp/skill-store/debug-samples/diagnostics/log-analysis",
                                ),
                                relative_path: "debug-samples/diagnostics/log-analysis".to_string(),
                                description: Some("Analyze diagnostic logs.".to_string()),
                            }),
                            SkillNode::Skill(Skill {
                                name: "reproduction".to_string(),
                                path: PathBuf::from(
                                    "/tmp/skill-store/debug-samples/diagnostics/reproduction",
                                ),
                                relative_path: "debug-samples/diagnostics/reproduction".to_string(),
                                description: Some("Reduce a failure to a small repro.".to_string()),
                            }),
                        ],
                    },
                    SkillNode::Folder {
                        name: "verification".to_string(),
                        relative_path: "debug-samples/verification".to_string(),
                        expanded: true,
                        children: vec![SkillNode::Skill(Skill {
                            name: "regression-test".to_string(),
                            path: PathBuf::from(
                                "/tmp/skill-store/debug-samples/verification/regression-test",
                            ),
                            relative_path: "debug-samples/verification/regression-test".to_string(),
                            description: Some("Lock in the regression test.".to_string()),
                        })],
                    },
                ],
            },
        ];
        app
    }

    fn rendered_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn home_view_explains_selected_skill() {
        let mut app = app_with_skill();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| ui(frame, &mut app)).unwrap();
        let text = rendered_text(&terminal);

        assert!(text.contains("[OFF]"));
        assert!(text.contains("Selected item"));
        assert!(text.contains("Description"));
        assert!(text.contains("Writes clear technical documentation."));
        assert!(text.contains("Skill  writer"));
        assert!(!text.contains("What happens next"));
    }

    #[test]
    fn narrow_home_view_keeps_the_detail_pane() {
        let mut app = app_with_skill();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| ui(frame, &mut app)).unwrap();
        let text = rendered_text(&terminal);

        assert!(text.contains("Selected item"));
        assert!(text.contains("Description"));
        assert!(text.contains("Status: OFF"));
        assert!(text.contains("Target: ~/.claude/skills/writer"));
    }

    #[test]
    fn nested_home_view_shows_tree_guides() {
        let mut app = app_with_nested_skills();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| ui(frame, &mut app)).unwrap();
        let text = rendered_text(&terminal);

        assert!(text.contains("managed skill tree"));
        assert!(text.contains("├─ ▼ diagnostics/"));
        assert!(text.contains("└─ ▼ verification/"));
        assert!(text.contains("├─ log-analysis"));
    }
}
