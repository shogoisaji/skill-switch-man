use crate::app::{App, CurrentScreen, VisibleItem};
use crate::config::Agent;
use crate::skills::SkillNode;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

const C_YELLOW: Color = Color::Rgb(255, 255, 0);
const C_GREEN: Color = Color::Rgb(154, 205, 50);

pub fn ui(f: &mut Frame, app: &mut App) {
    match app.current_screen {
        CurrentScreen::Settings | CurrentScreen::EditingSkillsSourcePath => {
            render_settings_screen(f, app, f.area())
        }
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
            Constraint::Length(3),
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
            .title("Tool")
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(accent)),
    );
    f.render_widget(tabs, chunks[0]);

    let list_view_height = chunks[1].height.saturating_sub(2) as usize;
    app.ensure_selection_visible(list_view_height);

    let lines = build_skill_lines(app);
    let content = if lines.is_empty() {
        vec![Line::from(Span::styled(
            "No skills found in the configured store.",
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
                .title(format!("{} Skills", app.active_agent.name()))
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        );
    f.render_widget(list, chunks[1]);

    let guide = Paragraph::new(home_guide_line())
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Guide")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        );
    f.render_widget(guide, chunks[2]);
}

fn build_skill_lines(app: &App) -> Vec<Line<'_>> {
    let visible = app.visible_items();
    visible
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            VisibleItem::SkillNode(node) => {
                let depth = calculate_depth(&app.skills, node.relative_path());
                build_skill_node_line(app, node, index, depth)
            }
            VisibleItem::UntrackedSkill(skill) => {
                build_untracked_line(skill, index == app.selected_index)
            }
        })
        .collect()
}

fn build_skill_node_line<'a>(
    app: &App,
    node: &'a SkillNode,
    index: usize,
    depth: usize,
) -> Line<'a> {
    let is_selected = index == app.selected_index;
    let cursor = if is_selected { ">" } else { " " };

    let base_style = if is_selected {
        Style::default().bg(Color::Rgb(50, 50, 50)).fg(C_YELLOW)
    } else {
        Style::default().fg(Color::White)
    };

    let (state_marker, state_style) = if let Some(skill) = node.skill() {
        let enabled = app
            .config
            .is_skill_enabled(app.active_agent, &skill.relative_path);
        let conflicts_with_untracked = app.has_untracked_conflict(app.active_agent, &skill.name);

        if enabled {
            (
                "●",
                Style::default().fg(C_GREEN).add_modifier(Modifier::BOLD),
            )
        } else if conflicts_with_untracked {
            (
                "△",
                if is_selected {
                    Style::default().bg(Color::Rgb(50, 50, 50)).fg(Color::Red)
                } else {
                    Style::default().fg(Color::Red)
                },
            )
        } else if is_selected {
            (
                "-",
                Style::default().bg(Color::Rgb(50, 50, 50)).fg(C_YELLOW),
            )
        } else {
            ("-", Style::default().fg(Color::Gray))
        }
    } else {
        (" ", base_style)
    };

    let (fold_marker, fold_style, icon) = if node.is_folder() {
        (
            if node.is_expanded() { "▼" } else { "▶︎" },
            if is_selected {
                Style::default().bg(Color::Rgb(50, 50, 50)).fg(C_YELLOW)
            } else {
                Style::default().fg(Color::White)
            },
            "📁",
        )
    } else {
        (" ", Style::default(), "📄")
    };

    let indent = "  ".repeat(depth);
    let mut spans = vec![
        Span::styled(format!("{} ", cursor), base_style),
        Span::styled(format!("{} ", state_marker), state_style),
        Span::styled(format!("{} ", fold_marker), fold_style),
        Span::styled(indent, base_style),
        Span::styled(format!("{} ", icon), base_style),
        Span::styled(node.name(), base_style.add_modifier(Modifier::BOLD)),
    ];

    if let Some(skill) = node.skill() {
        if let Some(description) = &skill.description {
            spans.push(Span::styled(
                format!("  {}", description),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    Line::from(spans)
}

fn build_untracked_line(skill: &crate::skills::UntrackedSkill, is_selected: bool) -> Line<'_> {
    let cursor = if is_selected { ">" } else { " " };
    let base_style = if is_selected {
        Style::default().bg(Color::Rgb(50, 50, 50)).fg(C_YELLOW)
    } else {
        Style::default().fg(Color::White)
    };
    let marker_style = if is_selected {
        Style::default().bg(Color::Rgb(50, 50, 50)).fg(Color::Red)
    } else {
        Style::default().fg(Color::Red)
    };
    let conflict = if skill.conflicts_with_source {
        "  name conflict"
    } else {
        ""
    };
    let source = skill
        .link_target
        .as_ref()
        .map(|target| format!("  symlink -> {}", target.display()))
        .unwrap_or_else(|| format!("  {}", skill.path.display()));

    Line::from(vec![
        Span::styled(format!("{} ", cursor), base_style),
        Span::styled("! ", marker_style),
        Span::styled("  ", base_style),
        Span::styled("📄 ", base_style),
        Span::styled(&skill.name, base_style.add_modifier(Modifier::BOLD)),
        Span::styled(conflict, Style::default().fg(Color::Red)),
        Span::styled(source, Style::default().fg(Color::DarkGray)),
    ])
}

fn calculate_depth(root_nodes: &[SkillNode], target_path: &str) -> usize {
    fn find_depth(nodes: &[SkillNode], target: &str, current_depth: usize) -> Option<usize> {
        for node in nodes {
            if node.relative_path() == target {
                return Some(current_depth);
            }
            if let SkillNode::Folder { children, .. } = node {
                if let Some(depth) = find_depth(children, target, current_depth + 1) {
                    return Some(depth);
                }
            }
        }
        None
    }

    find_depth(root_nodes, target_path, 0).unwrap_or(0)
}

fn render_settings_screen(f: &mut Frame, app: &App, area: Rect) {
    let accent = agent_color(app.active_agent);
    let value = if app.current_screen == CurrentScreen::EditingSkillsSourcePath {
        format!("{}|", app.input_buffer)
    } else {
        app.config.skills_source_dir.clone()
    };

    let content = Paragraph::new(settings_lines(value))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Skill Store Directory")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        );
    f.render_widget(content, area);
}

fn settings_lines(value: String) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(value), Line::from("")];
    lines.extend(Agent::ALL.into_iter().map(|agent| {
        Line::from(vec![
            Span::styled(
                format!("{} target: ", agent.display_name()),
                Style::default()
                    .fg(agent_color(agent))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("~/{}/skills", agent.home_dir_name())),
        ])
    }));
    lines
}

fn home_guide_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("←/→", Style::default().fg(Color::Cyan)),
        Span::raw(" Tool  "),
        Span::styled("↑/↓", Style::default().fg(Color::Cyan)),
        Span::raw(" Move  "),
        Span::styled("Space", Style::default().fg(Color::Cyan)),
        Span::raw(" Toggle  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" Save  "),
        Span::styled("Esc/Q", Style::default().fg(Color::Cyan)),
        Span::raw(" Quit"),
    ])
}

fn render_confirmation_dialog(f: &mut Frame, app: &App, area: Rect) {
    let accent = agent_color(app.active_agent);
    let popup = centered_rect(area, 72, 18);
    let popup_style = Style::default().bg(Color::Rgb(18, 18, 18)).fg(Color::White);
    let yes_style = if app.confirm_apply_yes {
        Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let no_style = if app.confirm_apply_yes {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)
    };

    let mut lines = vec![Line::from("Apply skill changes?"), Line::from("")];

    let changes = app.pending_changes();
    if changes.iter().all(|change| change.is_empty()) {
        lines.push(Line::from(Span::styled(
            "No skill changes.",
            Style::default().fg(Color::Gray),
        )));
    } else {
        for change in changes {
            if change.is_empty() {
                continue;
            }

            lines.push(Line::from(Span::styled(
                change.agent.name(),
                Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
            )));

            for skill in change.added {
                lines.push(Line::from(vec![
                    Span::styled("+ ", Style::default().fg(C_GREEN)),
                    Span::raw(skill.relative_path),
                ]));
            }

            for skill in change.removed {
                lines.push(Line::from(vec![
                    Span::styled("- ", Style::default().fg(Color::Red)),
                    Span::raw(skill.relative_path),
                ]));
            }

            lines.push(Line::from(""));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("["),
        Span::styled("Yes", yes_style),
        Span::raw("]   ["),
        Span::styled("No", no_style),
        Span::raw("]"),
    ]));

    let content = Paragraph::new(lines)
        .style(popup_style)
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .style(popup_style)
                .borders(Borders::ALL)
                .title("Confirm")
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
