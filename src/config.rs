use anyhow::{Context, Result};
use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    #[serde(
        default = "Config::default_skills_source_dir",
        alias = "source_dir",
        alias = "skill_store_dir"
    )]
    pub skills_source_dir: String,
    #[serde(default)]
    pub enabled_skills: EnabledSkills,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct EnabledSkills {
    #[serde(default)]
    pub claude: Vec<String>,
    #[serde(default)]
    pub codex: Vec<String>,
    #[serde(default)]
    pub opencode: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            skills_source_dir: Self::default_skills_source_dir(),
            enabled_skills: EnabledSkills::default(),
        }
    }
}

impl EnabledSkills {
    pub fn get(&self, agent: Agent) -> &[String] {
        match agent {
            Agent::Claude => &self.claude,
            Agent::Codex => &self.codex,
            Agent::OpenCode => &self.opencode,
        }
    }

    pub fn get_mut(&mut self, agent: Agent) -> &mut Vec<String> {
        match agent {
            Agent::Claude => &mut self.claude,
            Agent::Codex => &mut self.codex,
            Agent::OpenCode => &mut self.opencode,
        }
    }
}

impl Config {
    pub fn default_skills_source_dir() -> String {
        "~/.config/skill-switch-man/skill-store".to_string()
    }

    pub fn get_config_dir() -> Result<PathBuf> {
        let user_dirs =
            UserDirs::new().ok_or_else(|| anyhow::anyhow!("Failed to determine home directory"))?;
        Ok(user_dirs
            .home_dir()
            .join(".config")
            .join("skill-switch-man"))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::get_config_dir()?.join("settings.json"))
    }

    fn load_from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to deserialize config file: {}", path.display()))
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        if config_path.exists() {
            let config = Self::load_from_path(&config_path)?;
            config.ensure_managed_storage_exists()?;
            return Ok(config);
        }

        let config = Self::default();
        config.ensure_managed_storage_exists()?;
        config
            .save()
            .context("Failed to create default config file")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        self.ensure_managed_storage_exists()?;

        let config_dir = Self::get_config_dir()?;
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).with_context(|| {
                format!(
                    "Failed to create config directory: {}",
                    config_dir.display()
                )
            })?;
        }

        let config_path = config_dir.join("settings.json");
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;
        Ok(())
    }

    pub fn get_skills_source_dir(&self) -> PathBuf {
        expand_tilde(&self.skills_source_dir)
    }

    pub fn is_skill_enabled(&self, agent: Agent, skill_name: &str) -> bool {
        self.enabled_skills
            .get(agent)
            .iter()
            .any(|skill| skill == skill_name)
    }

    pub fn toggle_skill(&mut self, agent: Agent, skill_name: &str) {
        let skills = self.enabled_skills.get_mut(agent);
        if let Some(index) = skills.iter().position(|s| s == skill_name) {
            skills.remove(index);
        } else {
            skills.push(skill_name.to_string());
        }
    }

    fn ensure_managed_storage_exists(&self) -> Result<()> {
        let config_dir = Self::get_config_dir()?;
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).with_context(|| {
                format!(
                    "Failed to create config directory: {}",
                    config_dir.display()
                )
            })?;
        }

        let skills_dir = self.get_skills_source_dir();
        if !skills_dir.exists() {
            fs::create_dir_all(&skills_dir).with_context(|| {
                format!("Failed to create skill store: {}", skills_dir.display())
            })?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    OpenCode,
}

impl Agent {
    pub const ALL: [Agent; 3] = [Agent::Claude, Agent::Codex, Agent::OpenCode];

    pub fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|agent| *agent == self)
            .unwrap_or(0)
    }

    pub fn next(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    pub fn prev(self) -> Self {
        let index = self.index();
        if index == 0 {
            Self::from_index(Self::ALL.len() - 1)
        } else {
            Self::from_index(index - 1)
        }
    }

    pub fn target_dir(&self) -> Result<PathBuf> {
        Ok(agent_home_dir()?.join(self.home_dir_name()).join("skills"))
    }

    pub fn home_dir_name(&self) -> &'static str {
        match self {
            Agent::Claude => ".claude",
            Agent::Codex => ".codex",
            Agent::OpenCode => ".opencode",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Agent::Claude => "C",
            Agent::Codex => "X",
            Agent::OpenCode => "O",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Agent::Claude => "Claude",
            Agent::Codex => "Codex",
            Agent::OpenCode => "OpenCode",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::OpenCode => "OpenCode",
        }
    }

    pub fn accent_rgb(&self) -> (u8, u8, u8) {
        match self {
            Agent::Claude => (255, 165, 0),
            Agent::Codex => (154, 205, 50),
            Agent::OpenCode => (180, 120, 255),
        }
    }
}

fn agent_home_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("SKILL_SWITCH_MAN_HOME") {
        return Ok(PathBuf::from(home));
    }

    let user_dirs =
        UserDirs::new().ok_or_else(|| anyhow::anyhow!("Failed to determine home directory"))?;
    Ok(user_dirs.home_dir().to_path_buf())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(user_dirs) = UserDirs::new() {
            return user_dirs.home_dir().to_path_buf();
        }
    }

    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(user_dirs) = UserDirs::new() {
            return user_dirs.home_dir().join(stripped);
        }
    }

    PathBuf::from(path)
}

pub fn collapse_tilde(path: &Path) -> String {
    if let Some(user_dirs) = UserDirs::new() {
        let home = user_dirs.home_dir();
        if let Ok(relative) = path.strip_prefix(home) {
            return format!("~/{}", relative.display());
        }
    }

    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::{Agent, Config, EnabledSkills};

    #[test]
    fn enabled_skills_defaults_missing_agent_fields() {
        let config: Config = serde_json::from_str(
            r#"{
              "skills_source_dir": "/tmp/skills",
              "enabled_skills": {
                "claude": ["writer"]
              }
            }"#,
        )
        .unwrap();

        assert!(config.is_skill_enabled(Agent::Claude, "writer"));
        assert!(config.enabled_skills.get(Agent::Codex).is_empty());
        assert!(config.enabled_skills.get(Agent::OpenCode).is_empty());
    }

    #[test]
    fn source_dir_aliases_are_accepted() {
        let config: Config = serde_json::from_str(
            r#"{
              "skill_store_dir": "/tmp/custom-store",
              "enabled_skills": {}
            }"#,
        )
        .unwrap();

        assert_eq!(config.skills_source_dir, "/tmp/custom-store");
    }

    #[test]
    fn enabled_skills_accessors_are_agent_aware() {
        let mut enabled = EnabledSkills::default();
        enabled.get_mut(Agent::OpenCode).push("planner".to_string());

        assert_eq!(enabled.get(Agent::OpenCode), ["planner"]);
        assert!(enabled.get(Agent::Claude).is_empty());
    }

    #[test]
    fn agent_navigation_wraps_over_all_tools() {
        assert_eq!(Agent::Claude.prev(), Agent::OpenCode);
        assert_eq!(Agent::Claude.next(), Agent::Codex);
        assert_eq!(Agent::Codex.next(), Agent::OpenCode);
        assert_eq!(Agent::OpenCode.next(), Agent::Claude);
    }
}
