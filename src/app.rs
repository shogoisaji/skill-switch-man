use crate::config::{collapse_tilde, expand_tilde, Agent, Config};
use crate::skills::{
    capture_skill_links_for_configs, collect_existing_relative_paths, find_folder_by_path_mut,
    flatten_visible_nodes, list_skills, list_untracked_skills, restore_skill_links, sync_skills,
    Skill, SkillNode, UntrackedSkill,
};
use anyhow::Result;
use std::collections::BTreeSet;

#[derive(PartialEq, Clone, Copy, Debug, Default)]
pub enum CurrentScreen {
    #[default]
    Home,
    Settings,
    EditingSkillsSourcePath,
    Confirmation,
    Help,
}

pub struct App {
    pub config: Config,
    pub saved_config: Config,
    pub skills: Vec<SkillNode>,
    pub untracked_skills: Vec<AgentUntrackedSkills>,
    pub selected_index: usize,
    pub list_scroll_offset: usize,
    pub list_viewport_height: usize,
    pub active_agent: Agent,
    pub message: Option<String>,
    pub current_screen: CurrentScreen,
    pub input_buffer: String,
    pub confirm_apply_yes: bool,
    pub confirm_scroll_offset: usize,
    pub confirm_viewport_height: usize,
    pub hit_targets: HitTargets,
    pub(crate) help_return_screen: CurrentScreen,
}

impl Default for App {
    fn default() -> Self {
        Self {
            config: Config::default(),
            saved_config: Config::default(),
            skills: Vec::new(),
            untracked_skills: Vec::new(),
            selected_index: 0,
            list_scroll_offset: 0,
            list_viewport_height: 0,
            active_agent: Agent::Claude,
            message: None,
            current_screen: CurrentScreen::Home,
            input_buffer: String::new(),
            confirm_apply_yes: true,
            confirm_scroll_offset: 0,
            confirm_viewport_height: 0,
            hit_targets: HitTargets::default(),
            help_return_screen: CurrentScreen::Home,
        }
    }
}

impl App {
    pub fn new() -> Result<Self> {
        let mut config = Config::load()?;
        let skills = list_skills(&config.get_skills_source_dir())?;
        let existing = collect_existing_relative_paths(&skills);
        let message = match sync_skills(&config, &config, &skills) {
            Ok(pruned) => {
                let pruned_config = config.prune_missing_enabled_skills(&existing);
                if !pruned.is_empty() || !pruned_config.is_empty() {
                    let _ = config.save();
                }
                if !pruned.is_empty() {
                    Some(format!("Cleaned up dangling links: {}", pruned.join(", ")))
                } else {
                    None
                }
            }
            Err(error) => Some(format!("Startup sync failed: {}", error)),
        };
        let untracked_skills = load_untracked_skills(&config, &skills)?;

        Ok(Self {
            saved_config: config.clone(),
            config,
            skills,
            untracked_skills,
            selected_index: 0,
            list_scroll_offset: 0,
            list_viewport_height: 0,
            active_agent: Agent::Claude,
            message,
            current_screen: CurrentScreen::Home,
            input_buffer: String::new(),
            confirm_apply_yes: true,
            confirm_scroll_offset: 0,
            confirm_viewport_height: 0,
            hit_targets: HitTargets::default(),
            help_return_screen: CurrentScreen::Home,
        })
    }

    pub fn reload_data(&mut self) -> Result<()> {
        self.skills = list_skills(&self.config.get_skills_source_dir())?;
        self.untracked_skills = load_untracked_skills(&self.config, &self.skills)?;
        self.clamp_selection();
        Ok(())
    }

    pub fn visible_skills(&self) -> Vec<&SkillNode> {
        flatten_visible_nodes(&self.skills)
    }

    pub fn visible_items(&self) -> Vec<VisibleItem<'_>> {
        let mut items: Vec<_> = self
            .visible_skills()
            .into_iter()
            .map(VisibleItem::SkillNode)
            .collect();
        items.extend(
            self.active_untracked_skills()
                .iter()
                .map(VisibleItem::UntrackedSkill),
        );
        items
    }

    pub fn next_item(&mut self) {
        if self.current_screen != CurrentScreen::Home {
            return;
        }

        let len = self.visible_items().len();
        if len > 0 && self.selected_index + 1 < len {
            self.selected_index += 1;
        }
    }

    pub fn prev_item(&mut self) {
        if self.current_screen != CurrentScreen::Home {
            return;
        }

        let len = self.visible_items().len();
        if len == 0 {
            return;
        }

        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn next_agent(&mut self) {
        if self.current_screen == CurrentScreen::Home {
            self.active_agent = self.active_agent.next();
            self.selected_index = 0;
            self.list_scroll_offset = 0;
            self.message = None;
        }
    }

    pub fn prev_agent(&mut self) {
        if self.current_screen == CurrentScreen::Home {
            self.active_agent = self.active_agent.prev();
            self.selected_index = 0;
            self.list_scroll_offset = 0;
            self.message = None;
        }
    }

    pub fn toggle_current(&mut self) {
        match self.current_screen {
            CurrentScreen::Home => self.toggle_selected_skill(),
            CurrentScreen::Settings => self.start_editing_skills_source(),
            CurrentScreen::Confirmation => {
                self.confirm_apply_yes = !self.confirm_apply_yes;
            }
            CurrentScreen::EditingSkillsSourcePath | CurrentScreen::Help => {}
        }
    }

    fn toggle_selected_skill(&mut self) {
        let visible = self.visible_items();
        let Some(item) = visible.get(self.selected_index) else {
            return;
        };
        let VisibleItem::SkillNode(node) = item else {
            self.message = Some("Untracked skills cannot be toggled".to_string());
            return;
        };

        let is_folder = node.is_folder();
        let relative_path = node.relative_path().to_string();
        let name = node.name().to_string();
        let skill_relative_path = node.skill().map(|skill| skill.relative_path.clone());

        if is_folder {
            if let Some(folder) = find_folder_by_path_mut(&mut self.skills, &relative_path) {
                let expanded = folder.is_expanded();
                folder.set_expanded(!expanded);
                self.message = Some(format!(
                    "Folder {} {}",
                    name,
                    if expanded { "collapsed" } else { "expanded" }
                ));
                self.clamp_selection();
            }
            return;
        }

        if let Some(skill_path) = skill_relative_path {
            let currently_enabled = self.config.is_skill_enabled(self.active_agent, &skill_path);
            if !currently_enabled && self.has_untracked_conflict(self.active_agent, &name) {
                self.message = Some(format!(
                    "{} skill '{}' is untracked; remove or rename it before enabling",
                    self.active_agent.name(),
                    name
                ));
                return;
            }

            self.config.toggle_skill(self.active_agent, &skill_path);
            let enabled = self.config.is_skill_enabled(self.active_agent, &skill_path);
            self.message = Some(format!(
                "{} skill {} {}",
                self.active_agent.name(),
                name,
                if enabled { "enabled" } else { "disabled" }
            ));
        }
    }

    pub fn active_untracked_skills(&self) -> &[UntrackedSkill] {
        self.untracked_skills_for(self.active_agent)
    }

    pub fn has_untracked_conflict(&self, agent: Agent, skill_name: &str) -> bool {
        self.untracked_skills_for(agent)
            .iter()
            .any(|skill| skill.name == skill_name && skill.conflicts_with_source)
    }

    fn untracked_skills_for(&self, agent: Agent) -> &[UntrackedSkill] {
        self.untracked_skills
            .iter()
            .find(|entry| entry.agent == agent)
            .map(|entry| entry.skills.as_slice())
            .unwrap_or(&[])
    }

    pub fn request_apply(&mut self) {
        self.current_screen = CurrentScreen::Confirmation;
        self.confirm_apply_yes = true;
        self.confirm_scroll_offset = 0;
        self.message = None;
    }

    pub fn pending_change_items(&self) -> Vec<PendingChangeLine> {
        let changes = self.pending_changes();
        if changes.iter().all(|change| change.is_empty()) {
            return vec![PendingChangeLine::Empty];
        }

        let mut items = Vec::new();
        let mut first_group = true;
        for change in changes {
            if change.is_empty() {
                continue;
            }
            if !first_group {
                items.push(PendingChangeLine::Spacer);
            }
            first_group = false;
            items.push(PendingChangeLine::AgentHeader(change.agent));
            for skill in change.added {
                items.push(PendingChangeLine::Enable(skill.relative_path));
            }
            for skill in change.removed {
                items.push(PendingChangeLine::Disable(skill.relative_path));
            }
        }
        items
    }

    pub fn scroll_list_by(&mut self, delta: i32) {
        if self.current_screen != CurrentScreen::Home {
            return;
        }

        let len = self.visible_items().len();
        if len == 0 {
            self.list_scroll_offset = 0;
            self.selected_index = 0;
            return;
        }

        let viewport = self.list_viewport_height.max(1);
        let max_offset = len.saturating_sub(viewport);
        let next = (self.list_scroll_offset as i32 + delta).clamp(0, max_offset as i32) as usize;
        self.list_scroll_offset = next;

        if self.selected_index < next {
            self.selected_index = next;
        } else {
            let last_visible = next + viewport.saturating_sub(1);
            if self.selected_index > last_visible {
                self.selected_index = last_visible.min(len - 1);
            }
        }
    }

    pub fn page_list(&mut self, forward: bool) {
        let jump = self.list_viewport_height.max(1);
        for _ in 0..jump {
            if forward {
                self.next_item();
            } else {
                self.prev_item();
            }
        }
    }

    pub fn scroll_confirm_by(&mut self, delta: i32) {
        if self.current_screen != CurrentScreen::Confirmation {
            return;
        }

        let line_count = self.pending_change_items().len();
        let viewport = self.confirm_viewport_height.max(1);
        let max_offset = line_count.saturating_sub(viewport);
        self.confirm_scroll_offset =
            (self.confirm_scroll_offset as i32 + delta).clamp(0, max_offset as i32) as usize;
    }

    pub fn click_skill_list(&mut self, column: u16, row: u16) {
        if self.current_screen != CurrentScreen::Home {
            return;
        }
        let Some(area) = self.hit_targets.skill_list else {
            return;
        };
        let Some(relative_row) = area.relative_row(column, row) else {
            return;
        };

        let index = self
            .list_scroll_offset
            .saturating_add(relative_row as usize);
        let len = self.visible_items().len();
        if index >= len {
            return;
        }

        if self.selected_index == index {
            self.toggle_selected_skill();
        } else {
            self.selected_index = index;
            self.message = None;
        }
    }

    pub fn click_confirmation(&mut self, column: u16, row: u16) {
        if self.current_screen != CurrentScreen::Confirmation {
            return;
        }

        if self
            .hit_targets
            .apply_button
            .is_some_and(|area| area.contains(column, row))
        {
            self.confirm_apply_yes = true;
            let _ = self.confirm_apply();
        } else if self
            .hit_targets
            .cancel_button
            .is_some_and(|area| area.contains(column, row))
        {
            self.cancel_confirmation();
        }
    }

    pub fn execute_apply(&mut self) -> Result<()> {
        let snapshots =
            capture_skill_links_for_configs(&self.saved_config, &self.config, &self.skills)?;

        match sync_skills(&self.saved_config, &self.config, &self.skills) {
            Ok(pruned) => {
                let existing = collect_existing_relative_paths(&self.skills);
                let _pruned_config = self.config.prune_missing_enabled_skills(&existing);
                if let Err(error) = self.config.save() {
                    let rollback_result = restore_skill_links(&snapshots);
                    self.current_screen = CurrentScreen::Home;
                    self.message = Some(match rollback_result {
                        Ok(_) => format!("Error saving config: {}", error),
                        Err(rollback_error) => {
                            format!(
                                "Error saving config: {}; rollback failed: {}",
                                error, rollback_error
                            )
                        }
                    });
                    return Err(error);
                }

                self.saved_config = self.config.clone();
                self.current_screen = CurrentScreen::Home;
                if pruned.is_empty() {
                    self.message = Some("Changes applied successfully".to_string());
                } else {
                    self.message = Some(format!(
                        "Changes applied; cleaned up dangling links: {}",
                        pruned.join(", ")
                    ));
                }
                Ok(())
            }
            Err(error) => {
                let rollback_result = restore_skill_links(&snapshots);
                self.current_screen = CurrentScreen::Home;
                self.message = Some(match rollback_result {
                    Ok(_) => format!("Error: {}", error),
                    Err(rollback_error) => {
                        format!("Error: {}; rollback failed: {}", error, rollback_error)
                    }
                });
                Err(error)
            }
        }
    }

    pub fn pending_changes(&self) -> Vec<AgentSkillChanges> {
        Agent::ALL
            .into_iter()
            .map(|agent| {
                let saved = enabled_set(&self.saved_config, agent);
                let current = enabled_set(&self.config, agent);
                let added = current
                    .difference(&saved)
                    .filter_map(|path| find_skill(self.skills.as_slice(), path))
                    .cloned()
                    .collect();
                let removed = saved
                    .difference(&current)
                    .filter_map(|path| find_skill(self.skills.as_slice(), path))
                    .cloned()
                    .collect();

                AgentSkillChanges {
                    agent,
                    added,
                    removed,
                }
            })
            .collect()
    }

    pub fn confirm_apply(&mut self) -> bool {
        if self.confirm_apply_yes {
            self.execute_apply().is_ok()
        } else {
            self.cancel_confirmation();
            false
        }
    }

    pub fn cancel_confirmation(&mut self) {
        self.current_screen = CurrentScreen::Home;
        self.message = Some("Apply canceled".to_string());
    }

    pub fn enter_settings(&mut self) {
        self.current_screen = CurrentScreen::Settings;
        self.message = None;
    }

    pub fn exit_settings(&mut self) {
        self.current_screen = CurrentScreen::Home;
    }

    pub fn enter_help(&mut self) {
        self.help_return_screen = self.current_screen;
        self.current_screen = CurrentScreen::Help;
        self.message = None;
    }

    pub fn exit_help(&mut self) {
        self.current_screen = self.help_return_screen;
    }

    pub fn start_editing_skills_source(&mut self) {
        self.current_screen = CurrentScreen::EditingSkillsSourcePath;
        self.input_buffer = self.config.skills_source_dir.clone();
    }

    pub fn finish_editing_path(&mut self) {
        if self.current_screen != CurrentScreen::EditingSkillsSourcePath {
            return;
        }

        let trimmed = self.input_buffer.trim();
        if trimmed.is_empty() {
            self.message = Some("Skill store path cannot be empty".to_string());
            return;
        }

        let previous_path = self.config.skills_source_dir.clone();
        self.config.skills_source_dir = collapse_tilde(&expand_tilde(trimmed));
        self.input_buffer.clear();
        self.current_screen = CurrentScreen::Settings;

        if let Err(error) = self.config.save().and_then(|_| self.reload_data()) {
            self.config.skills_source_dir = previous_path;
            let rollback_result = self.config.save().and_then(|_| self.reload_data());
            self.message = Some(match rollback_result {
                Ok(_) => format!("Error reloading data: {}", error),
                Err(rollback_error) => {
                    format!(
                        "Error reloading data: {}; rollback failed: {}",
                        error, rollback_error
                    )
                }
            });
        } else {
            self.saved_config = self.config.clone();
            self.message = Some("Skill store path updated".to_string());
        }
    }

    pub fn cancel_editing(&mut self) {
        self.input_buffer.clear();
        self.current_screen = CurrentScreen::Settings;
    }

    pub fn handle_input_char(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    pub fn handle_backspace(&mut self) {
        self.input_buffer.pop();
    }

    fn clamp_selection(&mut self) {
        let len = self.visible_items().len();
        if len == 0 {
            self.selected_index = 0;
            self.list_scroll_offset = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }
    }

    pub fn ensure_selection_visible(&mut self, viewport_height: usize) {
        self.list_viewport_height = viewport_height;
        if viewport_height == 0 {
            self.list_scroll_offset = 0;
            return;
        }

        if self.selected_index < self.list_scroll_offset {
            self.list_scroll_offset = self.selected_index;
        } else {
            let last_visible = self.list_scroll_offset + viewport_height.saturating_sub(1);
            if self.selected_index > last_visible {
                self.list_scroll_offset = self.selected_index + 1 - viewport_height;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VisibleItem<'a> {
    SkillNode(&'a SkillNode),
    UntrackedSkill(&'a UntrackedSkill),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingChangeLine {
    Empty,
    Spacer,
    AgentHeader(Agent),
    Enable(String),
    Disable(String),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HitRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl HitRect {
    pub fn contains(self, column: u16, row: u16) -> bool {
        column >= self.x
            && row >= self.y
            && column < self.x.saturating_add(self.width)
            && row < self.y.saturating_add(self.height)
    }

    pub fn relative_row(self, column: u16, row: u16) -> Option<u16> {
        if self.contains(column, row) {
            Some(row - self.y)
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HitTargets {
    pub skill_list: Option<HitRect>,
    pub confirm_list: Option<HitRect>,
    pub apply_button: Option<HitRect>,
    pub cancel_button: Option<HitRect>,
}

#[derive(Debug, Clone)]
pub struct AgentSkillChanges {
    pub agent: Agent,
    pub added: Vec<Skill>,
    pub removed: Vec<Skill>,
}

#[derive(Debug, Clone)]
pub struct AgentUntrackedSkills {
    pub agent: Agent,
    pub skills: Vec<UntrackedSkill>,
}

impl AgentSkillChanges {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

fn enabled_set(config: &Config, agent: Agent) -> BTreeSet<String> {
    config.enabled_skills.get(agent).iter().cloned().collect()
}

fn find_skill<'a>(nodes: &'a [SkillNode], relative_path: &str) -> Option<&'a Skill> {
    for node in nodes {
        match node {
            SkillNode::Skill(skill) if skill.relative_path == relative_path => return Some(skill),
            SkillNode::Folder { children, .. } => {
                if let found @ Some(_) = find_skill(children, relative_path) {
                    return found;
                }
            }
            _ => {}
        }
    }
    None
}

fn load_untracked_skills(
    config: &Config,
    skills: &[SkillNode],
) -> Result<Vec<AgentUntrackedSkills>> {
    Agent::ALL
        .into_iter()
        .map(|agent| {
            Ok(AgentUntrackedSkills {
                agent,
                skills: list_untracked_skills(config, skills, agent)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::Skill;
    use std::path::PathBuf;

    fn skill(name: &str) -> SkillNode {
        SkillNode::Skill(Skill {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/skill-store/{name}")),
            relative_path: name.to_string(),
            description: None,
        })
    }

    fn app_with_skills(names: &[&str]) -> App {
        App {
            skills: names.iter().copied().map(skill).collect(),
            ..App::default()
        }
    }

    fn change_paths(changes: &[AgentSkillChanges], agent: Agent) -> (Vec<String>, Vec<String>) {
        let change = changes
            .iter()
            .find(|change| change.agent == agent)
            .expect("agent changes");
        (
            change
                .added
                .iter()
                .map(|skill| skill.relative_path.clone())
                .collect(),
            change
                .removed
                .iter()
                .map(|skill| skill.relative_path.clone())
                .collect(),
        )
    }

    #[test]
    fn pending_changes_lists_skills_in_stable_sorted_order() {
        let mut app = app_with_skills(&["zebra", "alpha", "middle", "writer", "planner"]);
        for name in ["zebra", "alpha", "middle"] {
            app.config.toggle_skill(Agent::Claude, name);
        }
        app.saved_config.toggle_skill(Agent::Claude, "planner");
        app.saved_config.toggle_skill(Agent::Claude, "writer");
        app.config.toggle_skill(Agent::Codex, "zebra");
        app.config.toggle_skill(Agent::Codex, "alpha");

        let first = app.pending_changes();
        let (added, removed) = change_paths(&first, Agent::Claude);
        assert_eq!(added, ["alpha", "middle", "zebra"]);
        assert_eq!(removed, ["planner", "writer"]);

        let (codex_added, codex_removed) = change_paths(&first, Agent::Codex);
        assert_eq!(codex_added, ["alpha", "zebra"]);
        assert!(codex_removed.is_empty());

        for _ in 0..30 {
            assert_eq!(
                change_paths(&app.pending_changes(), Agent::Claude),
                (added.clone(), removed.clone())
            );
            assert_eq!(
                change_paths(&app.pending_changes(), Agent::Codex),
                (codex_added.clone(), codex_removed.clone())
            );
        }
    }

    #[test]
    fn click_skill_list_selects_then_toggles() {
        let mut app = app_with_skills(&["alpha", "beta", "gamma"]);
        app.hit_targets.skill_list = Some(HitRect {
            x: 1,
            y: 4,
            width: 40,
            height: 10,
        });

        app.click_skill_list(2, 5);
        assert_eq!(app.selected_index, 1);
        assert!(!app.config.is_skill_enabled(Agent::Claude, "beta"));

        app.click_skill_list(2, 5);
        assert!(app.config.is_skill_enabled(Agent::Claude, "beta"));
        assert!(!app.config.is_skill_enabled(Agent::Claude, "alpha"));
    }

    #[test]
    fn click_outside_skill_list_is_ignored() {
        let mut app = app_with_skills(&["alpha", "beta"]);
        app.hit_targets.skill_list = Some(HitRect {
            x: 1,
            y: 4,
            width: 40,
            height: 10,
        });

        app.click_skill_list(50, 5);
        assert_eq!(app.selected_index, 0);
        assert!(app.config.enabled_skills.get(Agent::Claude).is_empty());
    }

    #[test]
    fn scroll_list_keeps_selection_in_viewport() {
        let names: Vec<String> = (0..20).map(|i| format!("s{i:02}")).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = app_with_skills(&name_refs);
        app.list_viewport_height = 5;
        app.selected_index = 0;

        app.scroll_list_by(3);
        assert_eq!(app.list_scroll_offset, 3);
        assert_eq!(app.selected_index, 3);

        app.scroll_list_by(100);
        assert_eq!(app.list_scroll_offset, 15);
        assert_eq!(app.selected_index, 15);
    }

    #[test]
    fn confirm_scroll_clamps_to_overflowing_changes() {
        let names: Vec<String> = (0..20).map(|i| format!("s{i:02}")).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = app_with_skills(&name_refs);
        for name in &name_refs {
            app.config.toggle_skill(Agent::Claude, name);
        }
        app.current_screen = CurrentScreen::Confirmation;
        app.confirm_viewport_height = 5;

        let items = app.pending_change_items();
        assert!(items.len() > 5);
        assert!(matches!(items[0], PendingChangeLine::AgentHeader(_)));
        assert_eq!(items[1], PendingChangeLine::Enable("s00".to_string()));

        app.scroll_confirm_by(100);
        assert_eq!(app.confirm_scroll_offset, items.len().saturating_sub(5));
        app.scroll_confirm_by(-1);
        assert_eq!(app.confirm_scroll_offset, items.len().saturating_sub(6));
    }

    #[test]
    fn click_cancel_closes_confirmation() {
        let mut app = app_with_skills(&["alpha"]);
        app.current_screen = CurrentScreen::Confirmation;
        app.hit_targets.cancel_button = Some(HitRect {
            x: 12,
            y: 20,
            width: 10,
            height: 1,
        });

        app.click_confirmation(14, 20);
        assert_eq!(app.current_screen, CurrentScreen::Home);
        assert_eq!(app.message.as_deref(), Some("Apply canceled"));
    }
}
