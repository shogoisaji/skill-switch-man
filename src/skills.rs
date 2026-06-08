use crate::config::{Agent, Config};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub path: PathBuf,
    pub relative_path: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UntrackedSkill {
    pub name: String,
    pub path: PathBuf,
    pub link_target: Option<PathBuf>,
    pub conflicts_with_source: bool,
}

#[derive(Debug, Clone)]
pub enum SkillNode {
    Folder {
        name: String,
        relative_path: String,
        children: Vec<SkillNode>,
        expanded: bool,
    },
    Skill(Skill),
}

impl SkillNode {
    pub fn name(&self) -> &str {
        match self {
            SkillNode::Folder { name, .. } => name,
            SkillNode::Skill(skill) => &skill.name,
        }
    }

    pub fn relative_path(&self) -> &str {
        match self {
            SkillNode::Folder { relative_path, .. } => relative_path,
            SkillNode::Skill(skill) => &skill.relative_path,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, SkillNode::Folder { .. })
    }

    pub fn skill(&self) -> Option<&Skill> {
        match self {
            SkillNode::Skill(skill) => Some(skill),
            _ => None,
        }
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        if let SkillNode::Folder {
            expanded: current, ..
        } = self
        {
            *current = expanded;
        }
    }

    pub fn is_expanded(&self) -> bool {
        match self {
            SkillNode::Folder { expanded, .. } => *expanded,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillLinkSnapshot {
    target: PathBuf,
    state: SkillLinkState,
}

#[derive(Debug, Clone)]
enum SkillLinkState {
    Missing,
    Managed(PathBuf),
    Other,
}

pub fn list_skills(source_dir: &Path) -> Result<Vec<SkillNode>> {
    let mut nodes = Vec::new();
    if !source_dir.exists() {
        return Ok(nodes);
    }

    build_skill_tree(source_dir, source_dir, &mut nodes)?;
    nodes.sort_by(|a, b| a.name().cmp(b.name()));
    Ok(nodes)
}

fn build_skill_tree(root_dir: &Path, current_dir: &Path, nodes: &mut Vec<SkillNode>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(current_dir)?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if name.starts_with('.') {
            continue;
        }

        let relative_path = path
            .strip_prefix(root_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                nodes.push(SkillNode::Skill(Skill {
                    name: name.to_string(),
                    path: path.clone(),
                    relative_path,
                    description: read_skill_description(&skill_md),
                }));
            } else {
                let mut children = Vec::new();
                build_skill_tree(root_dir, &path, &mut children)?;
                if !children.is_empty() {
                    nodes.push(SkillNode::Folder {
                        name: name.to_string(),
                        relative_path,
                        children,
                        expanded: true,
                    });
                }
            }
        }
    }

    Ok(())
}

pub fn sync_skills(saved_config: &Config, config: &Config, skills: &[SkillNode]) -> Result<()> {
    for agent in Agent::ALL {
        sync_agent_skills(saved_config, config, skills, agent)?;
    }
    Ok(())
}

fn sync_agent_skills(
    saved_config: &Config,
    config: &Config,
    skills: &[SkillNode],
    agent: Agent,
) -> Result<()> {
    let enabled_skills = collect_enabled_skills(config, skills, agent);
    let previously_enabled_skills = collect_enabled_skills(saved_config, skills, agent);
    let mut name_map: HashMap<String, &Skill> = HashMap::new();
    let source_root = config.get_skills_source_dir();

    for skill in &enabled_skills {
        if let Some(existing) = name_map.get(&skill.name) {
            return Err(anyhow::anyhow!(
                "Duplicate skill name '{}' found in: {} and {}",
                skill.name,
                existing.relative_path,
                skill.relative_path
            ));
        }
        name_map.insert(skill.name.clone(), skill);
    }

    let target_root = agent.target_dir()?;
    fs::create_dir_all(&target_root).with_context(|| {
        format!(
            "Failed to create {} skills dir: {}",
            agent.name(),
            target_root.display()
        )
    })?;

    for skill in &enabled_skills {
        let target = target_root.join(&skill.name);
        if fs::symlink_metadata(&target).is_ok()
            && !is_tracked_skill_link(saved_config, agent, &target, &skill.name)
        {
            return Err(anyhow::anyhow!(
                "{} skill '{}' conflicts with an untracked entry: {}",
                agent.name(),
                skill.name,
                target.display()
            ));
        }

        create_symlink(&skill.path, &target).with_context(|| {
            format!(
                "Failed to create {} symlink for {}",
                agent.name(),
                skill.name
            )
        })?;
    }

    let enabled_names: HashSet<String> = enabled_skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect();
    for skill in previously_enabled_skills {
        if enabled_names.contains(&skill.name) {
            continue;
        }

        let target = target_root.join(&skill.name);
        if is_managed_skill_link(&target, &source_root) {
            remove_symlink(&target).ok();
        }
    }

    Ok(())
}

fn is_tracked_skill_link(config: &Config, agent: Agent, target: &Path, name: &str) -> bool {
    if !config.is_skill_enabled(agent, name) {
        return false;
    }

    let Ok(metadata) = fs::symlink_metadata(target) else {
        return false;
    };
    if !is_link_metadata(target, &metadata) {
        return false;
    }

    let source_root = config.get_skills_source_dir();
    let Ok(source) = resolve_link_source(target) else {
        return false;
    };
    if !source.starts_with(&source_root) {
        return false;
    }

    source
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .map(|file_name| file_name == name)
        .unwrap_or(false)
}

pub fn list_untracked_skills(
    config: &Config,
    skills: &[SkillNode],
    agent: Agent,
) -> Result<Vec<UntrackedSkill>> {
    let target_root = agent.target_dir()?;
    if !target_root.exists() {
        return Ok(Vec::new());
    }

    let source_names = collect_skill_names(skills);
    let mut untracked = Vec::new();

    for entry in fs::read_dir(&target_root)
        .with_context(|| format!("Failed to read {} skills dir", agent.name()))?
        .flatten()
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let name = name.to_string();

        if name.starts_with('.') || is_tracked_skill_link(config, agent, &path, &name) {
            continue;
        }
        let link_target = untracked_link_target(&path);

        untracked.push(UntrackedSkill {
            conflicts_with_source: source_names.contains(&name),
            name,
            path,
            link_target,
        });
    }

    untracked.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(untracked)
}

fn collect_skill_names(nodes: &[SkillNode]) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_skill_names_recursive(nodes, &mut names);
    names
}

fn collect_skill_names_recursive(nodes: &[SkillNode], out: &mut HashSet<String>) {
    for node in nodes {
        match node {
            SkillNode::Skill(skill) => {
                out.insert(skill.name.clone());
            }
            SkillNode::Folder { children, .. } => collect_skill_names_recursive(children, out),
        }
    }
}

pub fn collect_enabled_skills(config: &Config, nodes: &[SkillNode], agent: Agent) -> Vec<Skill> {
    let mut skills = Vec::new();
    collect_enabled_skills_recursive(config, nodes, agent, &mut skills);
    skills
}

fn collect_enabled_skills_recursive(
    config: &Config,
    nodes: &[SkillNode],
    agent: Agent,
    out: &mut Vec<Skill>,
) {
    for node in nodes {
        match node {
            SkillNode::Skill(skill) if config.is_skill_enabled(agent, &skill.relative_path) => {
                out.push(skill.clone());
            }
            SkillNode::Skill(_) => {}
            SkillNode::Folder { children, .. } => {
                collect_enabled_skills_recursive(config, children, agent, out);
            }
        }
    }
}

pub fn find_folder_by_path_mut<'a>(
    nodes: &'a mut [SkillNode],
    path: &str,
) -> Option<&'a mut SkillNode> {
    for node in nodes {
        if node.relative_path() == path && node.is_folder() {
            return Some(node);
        }
        if let SkillNode::Folder { children, .. } = node {
            if let found @ Some(_) = find_folder_by_path_mut(children, path) {
                return found;
            }
        }
    }
    None
}

pub fn flatten_visible_nodes(nodes: &[SkillNode]) -> Vec<&SkillNode> {
    let mut result = Vec::new();
    flatten_visible_nodes_recursive(nodes, &mut result);
    result
}

fn flatten_visible_nodes_recursive<'a>(nodes: &'a [SkillNode], out: &mut Vec<&'a SkillNode>) {
    for node in nodes {
        out.push(node);
        if let SkillNode::Folder {
            expanded: true,
            children,
            ..
        } = node
        {
            flatten_visible_nodes_recursive(children, out);
        }
    }
}

pub fn capture_skill_links_for_configs(
    saved_config: &Config,
    config: &Config,
    skills: &[SkillNode],
) -> Result<Vec<SkillLinkSnapshot>> {
    let mut snapshots = Vec::new();
    let mut seen = HashSet::new();

    for agent in Agent::ALL {
        let target_root = agent.target_dir()?;
        let mut tracked = collect_enabled_skills(saved_config, skills, agent);
        tracked.extend(collect_enabled_skills(config, skills, agent));

        for skill in tracked {
            let target = target_root.join(&skill.name);
            if seen.insert(target.clone()) {
                snapshots.push(SkillLinkSnapshot {
                    state: capture_skill_link_state(&target)?,
                    target,
                });
            }
        }
    }

    Ok(snapshots)
}

pub fn restore_skill_links(snapshots: &[SkillLinkSnapshot]) -> Result<()> {
    for snapshot in snapshots {
        match &snapshot.state {
            SkillLinkState::Missing => {
                remove_symlink(&snapshot.target).with_context(|| {
                    format!(
                        "Failed to remove restored skill link: {}",
                        snapshot.target.display()
                    )
                })?;
            }
            SkillLinkState::Managed(source) => {
                create_symlink(source, &snapshot.target).with_context(|| {
                    format!(
                        "Failed to restore skill link: {}",
                        snapshot.target.display()
                    )
                })?;
            }
            SkillLinkState::Other => {}
        }
    }

    Ok(())
}

fn read_skill_description(skill_md_path: &Path) -> Option<String> {
    let content = fs::read_to_string(skill_md_path).ok()?;
    for line in content.lines() {
        if let Some(description) = line.trim().strip_prefix("description:") {
            let description = description
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !description.is_empty() {
                return Some(description);
            }
        }
    }
    None
}

fn capture_skill_link_state(target: &Path) -> Result<SkillLinkState> {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return Ok(SkillLinkState::Missing);
    };

    if !is_link_metadata(target, &metadata) {
        return Ok(SkillLinkState::Other);
    }

    Ok(SkillLinkState::Managed(resolve_link_source(target)?))
}

fn is_managed_skill_link(target: &Path, source_root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return false;
    };
    if !is_link_metadata(target, &metadata) {
        return false;
    }

    resolve_link_source(target)
        .map(|source| source.starts_with(source_root))
        .unwrap_or(false)
}

fn untracked_link_target(target: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(target).ok()?;
    if !is_link_metadata(target, &metadata) {
        return None;
    }
    resolve_link_source(target).ok()
}

fn create_symlink(source: &Path, target: &Path) -> Result<()> {
    if source == target {
        return Ok(());
    }

    if let Ok(metadata) = fs::symlink_metadata(target) {
        if is_link_metadata(target, &metadata) {
            remove_existing_link(target, &metadata)?;
        } else {
            return Err(anyhow::anyhow!(
                "Target exists and is not a symlink: {}",
                target.display()
            ));
        }
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)?;
    }

    #[cfg(windows)]
    {
        junction::create(source, target)
            .context("Failed to create directory junction on Windows")?;
    }

    Ok(())
}

fn is_link_metadata(target: &Path, metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_type().is_symlink() || junction::exists(target).unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        let _ = target;
        metadata.file_type().is_symlink()
    }
}

fn resolve_link_source(target: &Path) -> Result<PathBuf> {
    if let Ok(path) = fs::read_link(target) {
        return Ok(if path.is_absolute() {
            path
        } else {
            target.parent().unwrap_or_else(|| Path::new(".")).join(path)
        });
    }

    fs::canonicalize(target)
        .with_context(|| format!("Failed to resolve link target: {}", target.display()))
}

fn remove_symlink(target: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return Ok(());
    };

    if is_link_metadata(target, &metadata) {
        remove_existing_link(target, &metadata)?;
    }

    Ok(())
}

fn remove_existing_link(target: &Path, metadata: &fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        let _ = metadata;
        fs::remove_file(target)?;
    }

    #[cfg(windows)]
    {
        if metadata.is_dir() {
            fs::remove_dir(target).or_else(|_| fs::remove_file(target))?;
        } else {
            fs::remove_file(target)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        collect_enabled_skills, list_skills, list_untracked_skills, sync_skills, SkillNode,
    };
    use crate::config::{Agent, Config, EnabledSkills};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "skill-switch-man-test-{}-{}",
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
            .map(|description| format!("description: {}\n", description))
            .unwrap_or_else(|| "# Skill\n".to_string());
        fs::write(dir.join("SKILL.md"), content).unwrap();
        dir
    }

    fn config_for(source_dir: &Path) -> Config {
        Config {
            skills_source_dir: source_dir.to_string_lossy().to_string(),
            enabled_skills: EnabledSkills::default(),
        }
    }

    #[cfg(unix)]
    fn symlink_dir(source: &Path, target: &Path) {
        std::os::unix::fs::symlink(source, target).unwrap();
    }

    #[test]
    fn list_skills_ignores_hidden_entries_and_builds_nested_folders() {
        let root = temp_root("list-skills");
        write_skill(&root, "top", Some("Top skill"));
        write_skill(&root, "group/nested", None);
        write_skill(&root, ".hidden", None);

        let nodes = list_skills(&root).unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().any(|node| node.name() == "top"));

        let group = nodes.iter().find(|node| node.name() == "group").unwrap();
        let SkillNode::Folder { children, .. } = group else {
            panic!("group should be a folder");
        };
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name(), "nested");
    }

    #[test]
    fn collect_enabled_skills_uses_relative_paths() {
        let root = temp_root("collect-enabled");
        write_skill(&root, "group/nested", None);
        let nodes = list_skills(&root).unwrap();
        let mut config = config_for(&root);
        config.toggle_skill(Agent::Codex, "group/nested");

        let enabled = collect_enabled_skills(&config, &nodes, Agent::Codex);
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "nested");
        assert_eq!(enabled[0].relative_path, "group/nested");
    }

    #[test]
    #[cfg(unix)]
    fn untracked_includes_external_symlink_and_reports_name_conflict() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("untracked-symlink");
        let store = root.join("store");
        let home = root.join("home");
        let external = root.join("external");
        write_skill(&store, "writer", None);
        write_skill(&external, "writer", None);
        fs::create_dir_all(home.join(".codex/skills")).unwrap();
        symlink_dir(&external.join("writer"), &home.join(".codex/skills/writer"));

        std::env::set_var("SKILL_SWITCH_MAN_HOME", &home);
        let config = config_for(&store);
        let nodes = list_skills(&store).unwrap();
        let untracked = list_untracked_skills(&config, &nodes, Agent::Codex).unwrap();
        std::env::remove_var("SKILL_SWITCH_MAN_HOME");

        assert_eq!(untracked.len(), 1);
        assert_eq!(untracked[0].name, "writer");
        assert!(untracked[0].conflicts_with_source);
        assert_eq!(untracked[0].link_target, Some(external.join("writer")));
    }

    #[test]
    #[cfg(unix)]
    fn sync_refuses_to_overwrite_untracked_conflict() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("sync-conflict");
        let store = root.join("store");
        let home = root.join("home");
        write_skill(&store, "writer", None);
        fs::create_dir_all(home.join(".codex/skills/writer")).unwrap();

        std::env::set_var("SKILL_SWITCH_MAN_HOME", &home);
        let saved_config = config_for(&store);
        let mut config = config_for(&store);
        config.toggle_skill(Agent::Codex, "writer");
        let nodes = list_skills(&store).unwrap();
        let error = sync_skills(&saved_config, &config, &nodes).unwrap_err();
        std::env::remove_var("SKILL_SWITCH_MAN_HOME");

        assert!(error
            .to_string()
            .contains("conflicts with an untracked entry"));
    }

    #[test]
    #[cfg(unix)]
    fn sync_removes_previously_tracked_link_when_disabled() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root("sync-disable");
        let store = root.join("store");
        let home = root.join("home");
        write_skill(&store, "writer", None);
        fs::create_dir_all(home.join(".codex/skills")).unwrap();
        symlink_dir(&store.join("writer"), &home.join(".codex/skills/writer"));

        std::env::set_var("SKILL_SWITCH_MAN_HOME", &home);
        let mut saved_config = config_for(&store);
        saved_config.toggle_skill(Agent::Codex, "writer");
        let config = config_for(&store);
        let nodes = list_skills(&store).unwrap();
        sync_skills(&saved_config, &config, &nodes).unwrap();
        std::env::remove_var("SKILL_SWITCH_MAN_HOME");

        assert!(!home.join(".codex/skills/writer").exists());
    }
}
