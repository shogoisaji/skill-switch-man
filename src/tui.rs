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
            Event::Paste(text) => {
                if handle_paste_event(app, &text) {
                    break;
                }
            }
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
            _ if is_space_key(key) => app.toggle_current(),
            _ if is_enter_key(key) => app.request_apply(),
            _ => {}
        },
        CurrentScreen::Settings => match key.code {
            KeyCode::Esc => app.exit_settings(),
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
}
