pub mod add_store;
pub mod app;
pub mod config;
pub mod skills;
pub mod tui;
pub mod ui;
pub mod view_html;

use anyhow::Result;
use app::App;

const HELP: &str = "\
skillswitchman

TUI for enabling shared agent skills for Claude Code, Codex, and OpenCode.

USAGE:
  skillswitchman [OPTIONS]
  skillswitchman <COMMAND>

COMMANDS:
  add-store <URL>    Import skills from a GitHub repository into the skill store
  view-html          Generate an HTML status page and open it in the browser

OPTIONS:
  -h, --help       Show this help message
  -V, --version    Show version

FILES:
  ~/.config/skill-switch-man/settings.json

SETTINGS:
  skills_source_dir  Directory where reusable skills are stored.
                     Default: ~/.config/skill-switch-man/skill-store

EXAMPLE settings.json:
  {
    \"skills_source_dir\": \"~/src/skill-store\",
    \"enabled_skills\": {
      \"claude\": [\"browser-use\"],
      \"codex\": [\"browser-use\", \"imagegen\"],
      \"opencode\": []
    }
  }

KEYS:
  Left/Right, h/l   Switch tool tab
  Up/Down, j/k      Move selection
  Space             Toggle selected skill for current tool
  Enter             Show changes and save
  Esc, q            Quit
";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("-h" | "--help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("skillswitchman {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("add-store") => {
            let url = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: skillswitchman add-store <URL>"))?;
            add_store::run(url)
        }
        Some("view-html") => view_html::run(),
        Some(option) if option.starts_with('-') => {
            eprintln!("Unknown option: {option}");
            eprintln!("Run `skillswitchman --help` for usage.");
            std::process::exit(2);
        }
        Some(cmd) => {
            eprintln!("Unknown command: {cmd}");
            eprintln!("Run `skillswitchman --help` for usage.");
            std::process::exit(2);
        }
        None => {
            let mut app = App::new()?;
            tui::run(&mut app)?;
            Ok(())
        }
    }
}
