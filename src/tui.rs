use crate::app::{App, CurrentScreen};
use crate::ui::ui;
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
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
            Event::Mouse(mouse) => {
                handle_mouse_event(app, mouse);
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
            KeyCode::PageDown => app.page_list(true),
            KeyCode::PageUp => app.page_list(false),
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
            KeyCode::Up | KeyCode::Char('k') => app.scroll_confirm_by(-1),
            KeyCode::Down | KeyCode::Char('j') => app.scroll_confirm_by(1),
            KeyCode::PageUp => {
                app.scroll_confirm_by(-(app.confirm_viewport_height.max(1) as i32));
            }
            KeyCode::PageDown => {
                app.scroll_confirm_by(app.confirm_viewport_height.max(1) as i32);
            }
            _ if is_space_key(key) => app.toggle_current(),
            _ if is_enter_key(key) => {
                let _ = app.confirm_apply();
            }
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
            CurrentScreen::Confirmation => {
                let _ = app.confirm_apply();
            }
            CurrentScreen::EditingSkillsSourcePath => {}
            CurrentScreen::Help => {}
        },
        _ => {}
    }

    false
}

fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    const WHEEL_DELTA: i32 = 3;
    match app.current_screen {
        CurrentScreen::Home => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                app.click_skill_list(mouse.column, mouse.row);
            }
            MouseEventKind::ScrollUp => app.scroll_list_by(-WHEEL_DELTA),
            MouseEventKind::ScrollDown => app.scroll_list_by(WHEEL_DELTA),
            _ => {}
        },
        CurrentScreen::Confirmation => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                app.click_confirmation(mouse.column, mouse.row);
            }
            MouseEventKind::ScrollUp => app.scroll_confirm_by(-WHEEL_DELTA),
            MouseEventKind::ScrollDown => app.scroll_confirm_by(WHEEL_DELTA),
            _ => {}
        },
        _ => {}
    }
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
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

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
        App::default()
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

    #[test]
    fn applying_from_confirmation_does_not_quit() {
        let mut app = empty_app();
        app.current_screen = CurrentScreen::Confirmation;
        app.confirm_apply_yes = false;

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ));
        assert_eq!(app.current_screen, CurrentScreen::Home);
    }

    #[test]
    fn mouse_click_selects_skill_list_row() {
        use crate::app::HitRect;
        use crate::config::Agent;
        use crate::skills::{Skill, SkillNode};
        use std::path::PathBuf;

        let mut app = empty_app();
        app.skills = vec![
            SkillNode::Skill(Skill {
                name: "alpha".to_string(),
                path: PathBuf::from("/tmp/skill-store/alpha"),
                relative_path: "alpha".to_string(),
                description: None,
            }),
            SkillNode::Skill(Skill {
                name: "beta".to_string(),
                path: PathBuf::from("/tmp/skill-store/beta"),
                relative_path: "beta".to_string(),
                description: None,
            }),
        ];
        app.hit_targets.skill_list = Some(HitRect {
            x: 1,
            y: 4,
            width: 40,
            height: 8,
        });

        super::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.selected_index, 1);
        assert!(!app.config.is_skill_enabled(Agent::Claude, "beta"));

        super::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(app.config.is_skill_enabled(Agent::Claude, "beta"));
    }

    #[test]
    fn mouse_wheel_scrolls_home_list() {
        use crate::skills::{Skill, SkillNode};
        use std::path::PathBuf;

        let mut app = empty_app();
        app.skills = (0..20)
            .map(|i| {
                let name = format!("s{i:02}");
                SkillNode::Skill(Skill {
                    name: name.clone(),
                    path: PathBuf::from(format!("/tmp/skill-store/{name}")),
                    relative_path: name,
                    description: None,
                })
            })
            .collect();
        app.list_viewport_height = 5;

        super::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 2,
                row: 6,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.list_scroll_offset, 3);
        assert_eq!(app.selected_index, 3);
    }

    #[test]
    fn confirmation_up_down_scrolls_change_list() {
        use crate::skills::{Skill, SkillNode};
        use std::path::PathBuf;

        let mut app = empty_app();
        app.skills = (0..20)
            .map(|i| {
                let name = format!("s{i:02}");
                SkillNode::Skill(Skill {
                    name: name.clone(),
                    path: PathBuf::from(format!("/tmp/skill-store/{name}")),
                    relative_path: name,
                    description: None,
                })
            })
            .collect();
        for i in 0..20 {
            app.config
                .toggle_skill(crate::config::Agent::Claude, &format!("s{i:02}"));
        }
        app.current_screen = CurrentScreen::Confirmation;
        app.confirm_viewport_height = 4;

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
        ));
        assert_eq!(app.confirm_scroll_offset, 1);

        assert!(!super::handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
        ));
        assert_eq!(app.confirm_scroll_offset, 0);
    }
}
