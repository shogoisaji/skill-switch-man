pub mod app;
pub mod config;
pub mod skills;
pub mod tui;
pub mod ui;

use anyhow::Result;
use app::App;

const HELP: &str = "\
skillswitchman

TUI for enabling shared agent skills for Claude Code, Codex, and OpenCode.

USAGE:
  skillswitchman [OPTIONS]

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
    match std::env::args().nth(1).as_deref() {
        Some("-h" | "--help") => {
            print!("{HELP}");
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("skillswitchman {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(option) => {
            eprintln!("Unknown option: {option}");
            eprintln!("Run `skillswitchman --help` for usage.");
            std::process::exit(2);
        }
        None => {}
    }

    let mut app = App::new()?;
    tui::run(&mut app)?;
    Ok(())
}
