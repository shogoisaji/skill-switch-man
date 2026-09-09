use crate::app::{App, CurrentScreen};
use crate::ui::ui;
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

pub fn run(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_event_loop(app, &mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

fn run_event_loop(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    loop {
        terminal.draw(|frame| ui(frame, app))?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if matches!(key.kind, KeyEventKind::Release) {
                    continue;
                }

                if handle_key_event(app, key) {
                    break;
                }
            }
            Event::Paste(text) if handle_paste_event(app, &text) => {
                break;
            }
            Event::Paste(_) => {}
            _ => {}
        }
    }

    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent) -> bool {
    match app.current_screen {
        CurrentScreen::EditingSkillsSourcePath => match key.code {
            _ if is_enter_key(key) => app.finish_editing_path(),
            KeyCode::Esc => app.cancel_editing(),
            KeyCode::Backspace => app.handle_backspace(),
            KeyCode::Char(c) => app.handle_input_char(c),
            _ => {}
        },
        CurrentScreen::Home => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Down | KeyCode::Char('j') => app.next_item(),
            KeyCode::Up | KeyCode::Char('k') => app.prev_item(),
            KeyCode::Left | KeyCode::Char('h') => app.prev_agent(),
            KeyCode::Right | KeyCode::Char('l') => app.next_agent(),
            KeyCode::Char('s') | KeyCode::Char('S') => app.enter_settings(),
            KeyCode::Char('?') | KeyCode::F(1) => app.enter_help(),
            _ if is_space_key(key) => app.toggle_current(),
            _ if is_enter_key(key) => app.request_apply(),
            _ => {}
        },
        CurrentScreen::Settings => match key.code {
            KeyCode::Esc => app.exit_settings(),
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            KeyCode::Char('?') | KeyCode::F(1) => app.enter_help(),
            _ if is_enter_key(key) || is_space_key(key) => app.toggle_current(),
            _ => {}
        },
        CurrentScreen::Confirmation => match key.code {
            KeyCode::Esc => app.cancel_confirmation(),
            KeyCode::Left | KeyCode::Char('h') => app.confirm_apply_yes = true,
            KeyCode::Right | KeyCode::Char('l') => app.confirm_apply_yes = false,
            KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_apply_yes = true,
            KeyCode::Char('n') | KeyCode::Char('N') => app.confirm_apply_yes = false,
            _ if is_space_key(key) => app.toggle_current(),
            _ if is_enter_key(key) => return app.confirm_apply(),
            _ => {}
        },
        CurrentScreen::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1) => app.exit_help(),
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            _ => {}
        },
    }

    false
}

fn handle_paste_event(app: &mut App, text: &str) -> bool {
    if app.current_screen == CurrentScreen::EditingSkillsSourcePath {
        match text {
            "\n" | "\r" | "\r\n" => app.finish_editing_path(),
            _ => {
                for c in text.chars() {
                    app.handle_input_char(c);
                }
            }
        }
        return false;
    }

    match text {
        " " | "\u{3000}" | "\u{00a0}" => {
            if matches!(
                app.current_screen,
                CurrentScreen::Home | CurrentScreen::Settings | CurrentScreen::Confirmation
            ) {
                app.toggle_current();
            }
        }
        "\n" | "\r" | "\r\n" => match app.current_screen {
            CurrentScreen::Home => app.request_apply(),
            CurrentScreen::Settings => app.toggle_current(),
            CurrentScreen::Confirmation => return app.confirm_apply(),
            CurrentScreen::EditingSkillsSourcePath => {}
            CurrentScreen::Help => {}
        },
        _ => {}
    }

    false
}

fn is_enter_key(key: KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r')
    ) || (key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('j')))
}

fn is_space_key(key: KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Char(' ') | KeyCode::Char('\u{3000}') | KeyCode::Char('\u{00a0}')
    )
}

#[cfg(test)]
mod tests {
    use super::{is_enter_key, is_space_key};
    use crate::app::{App, CurrentScreen};
    use crate::config::{Agent, Config};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn enter_variants_are_supported() {
        assert!(is_enter_key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        )));
        assert!(is_enter_key(KeyEvent::new(
            KeyCode::Char('\n'),
            KeyModifiers::NONE
        )));
        assert!(is_enter_key(KeyEvent::new(
            KeyCode::Char('\r'),
            KeyModifiers::NONE
        )));
        assert!(is_enter_key(KeyEvent::new(
            KeyCode::Char('m'),
            KeyModifiers::CONTROL
        )));
        assert!(is_enter_key(KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL
        )));
    }

    #[test]
    fn space_detection_only_matches_space() {
        assert!(is_space_key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::NONE
        )));
        assert!(is_space_key(KeyEvent::new(
            KeyCode::Char('\u{3000}'),
            KeyModifiers::NONE
        )));
        assert!(!is_space_key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        )));
        assert!(!is_space_key(KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::NONE
        )));
    }

    fn empty_app() -> App {
        App {
            config: Config::default(),
            saved_config: Config::default(),
            skills: Vec::new(),
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

    #[test]
    fn settings_shortcut_opens_settings() {
        let mut app = empty_app();

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)
        ));
        assert_eq!(app.current_screen, CurrentScreen::Settings);
    }

    #[test]
    fn help_shortcut_opens_and_escape_closes_help() {
        let mut app = empty_app();

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)
        ));
        assert_eq!(app.current_screen, CurrentScreen::Help);

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));
        assert_eq!(app.current_screen, CurrentScreen::Home);
    }

    #[test]
    fn help_returns_to_the_screen_that_opened_it() {
        let mut app = empty_app();
        app.enter_settings();

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)
        ));
        assert_eq!(app.current_screen, CurrentScreen::Help);

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));
        assert_eq!(app.current_screen, CurrentScreen::Settings);
    }
}
