use crate::config::{collapse_tilde, expand_tilde, Agent, Config};
use crate::skills::{
    capture_skill_links_for_configs, collect_existing_relative_paths, find_folder_by_path_mut,
    flatten_visible_nodes, list_skills, list_untracked_skills, restore_skill_links, sync_skills,
    Skill, SkillNode, UntrackedSkill,
};
use anyhow::Result;
use std::collections::HashSet;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum CurrentScreen {
    Home,
    Settings,
    EditingSkillsSourcePath,
    Confirmation,
}

pub struct App {
    pub config: Config,
    pub saved_config: Config,
    pub skills: Vec<SkillNode>,
    pub untracked_skills: Vec<AgentUntrackedSkills>,
    pub selected_index: usize,
    pub list_scroll_offset: usize,
    pub active_agent: Agent,
    pub message: Option<String>,
    pub current_screen: CurrentScreen,
    pub input_buffer: String,
    pub confirm_apply_yes: bool,
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
            active_agent: Agent::Claude,
            message,
            current_screen: CurrentScreen::Home,
            input_buffer: String::new(),
            confirm_apply_yes: true,
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
            CurrentScreen::EditingSkillsSourcePath => {}
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
        self.message = None;
    }

    pub fn execute_apply(&mut self) -> Result<()> {
        let snapshots =
            capture_skill_links_for_configs(&self.saved_config, &self.config, &self.skills)?;

        match sync_skills(&self.saved_config, &self.config, &self.skills) {
            Ok(pruned) => {
                let existing = collect_existing_relative_paths(&self.skills);
                let pruned_config = self.config.prune_missing_enabled_skills(&existing);
                if !pruned.is_empty() || !pruned_config.is_empty() {
                    let _ = self.config.save();
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

fn enabled_set(config: &Config, agent: Agent) -> HashSet<String> {
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
