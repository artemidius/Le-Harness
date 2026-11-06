use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use thiserror::Error;

use crate::event::RuntimeEvent;
use crate::session::SessionSnapshot;

#[derive(Debug, Error)]
pub enum TuiError {
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiAction {
    Previous,
    Next,
    First,
    Last,
    ToggleHelp,
    Quit,
    None,
}

#[derive(Debug, Clone)]
pub struct TuiState {
    session: SessionSnapshot,
    selected: usize,
    show_help: bool,
    quit: bool,
}

impl TuiState {
    pub fn new(session: SessionSnapshot) -> Self {
        Self {
            session,
            selected: 0,
            show_help: false,
            quit: false,
        }
    }

    pub fn apply(&mut self, action: TuiAction) {
        let last = self.session.events.len().saturating_sub(1);
        match action {
            TuiAction::Previous => self.selected = self.selected.saturating_sub(1),
            TuiAction::Next => self.selected = (self.selected + 1).min(last),
            TuiAction::First => self.selected = 0,
            TuiAction::Last => self.selected = last,
            TuiAction::ToggleHelp => self.show_help = !self.show_help,
            TuiAction::Quit => self.quit = true,
            TuiAction::None => {}
        }
    }

    pub fn selected_event(&self) -> Option<&RuntimeEvent> {
        self.session.events.get(self.selected)
    }
}

pub fn run(session: SessionSnapshot) -> Result<(), TuiError> {
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let mut state = TuiState::new(session);

    while !state.quit {
        terminal.draw(|frame| render(frame, &state))?;
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            state.apply(key_action(key.code));
        }
    }
    Ok(())
}

fn key_action(code: KeyCode) -> TuiAction {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => TuiAction::Quit,
        KeyCode::Char('?') => TuiAction::ToggleHelp,
        KeyCode::Up | KeyCode::Char('k') => TuiAction::Previous,
        KeyCode::Down | KeyCode::Char('j') => TuiAction::Next,
        KeyCode::Home | KeyCode::Char('g') => TuiAction::First,
        KeyCode::End | KeyCode::Char('G') => TuiAction::Last,
        _ => TuiAction::None,
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn render(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "Le Harness",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  session {}  {} events",
            state.session.header.id,
            state.session.events.len()
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Trace"));
    frame.render_widget(title, outer[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(outer[1]);
    render_events(frame, body[0], state);
    render_details(frame, body[1], state);

    let answer = Paragraph::new(state.session.footer.answer.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Terminal answer"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(answer, outer[2]);

    let footer = if state.show_help {
        "↑/k previous  ↓/j next  g first  G last  ? hide help  q quit"
    } else {
        "↑/↓ navigate  ? help  q quit"
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        outer[3],
    );
}

fn render_events(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let items: Vec<ListItem<'_>> = state
        .session
        .events
        .iter()
        .map(|event| ListItem::new(format!("{:>4}  {}", event.sequence, event.kind)))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Events"))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut list_state = ListState::default().with_selected(Some(state.selected));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_details(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let details = state
        .selected_event()
        .and_then(|event| serde_json::to_string_pretty(event).ok())
        .unwrap_or_else(|| "No runtime events".into());
    let status = serde_json::to_value(&state.session.footer.status)
        .ok()
        .and_then(|value| {
            value
                .get("state")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".into());
    let text = Text::from(format!("status: {status}\n\n{details}"));
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Selected event"),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::backend::TestBackend;

    use super::*;
    use crate::runtime::AgentStatus;
    use crate::session::{SESSION_SCHEMA_VERSION, SessionFooter, SessionHeader};

    fn snapshot() -> SessionSnapshot {
        SessionSnapshot {
            header: SessionHeader {
                schema_version: SESSION_SCHEMA_VERSION,
                id: "session-1".into(),
                parent_session_id: None,
                created_unix_ms: 1,
                prompt: "fix it".into(),
                repository_root: PathBuf::from("/repo"),
            },
            events: vec![
                RuntimeEvent::new(1, "session.started", serde_json::json!({})),
                RuntimeEvent::new(2, "model.finished", serde_json::json!({"reason":"stop"})),
            ],
            footer: SessionFooter {
                status: AgentStatus::Completed {
                    finish_reason: "stop".into(),
                },
                answer: "The repair is complete.".into(),
                model_requests: 1,
                tool_calls: 0,
            },
        }
    }

    #[test]
    fn reducer_clamps_navigation_and_toggles_help() {
        let mut state = TuiState::new(snapshot());
        state.apply(TuiAction::Previous);
        assert_eq!(state.selected, 0);
        state.apply(TuiAction::Last);
        state.apply(TuiAction::Next);
        assert_eq!(state.selected, 1);
        state.apply(TuiAction::ToggleHelp);
        assert!(state.show_help);
        state.apply(TuiAction::Quit);
        assert!(state.quit);
    }

    #[test]
    fn renders_trace_answer_and_selected_payload() {
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = TuiState::new(snapshot());
        terminal.draw(|frame| render(frame, &state)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("Le Harness"));
        assert!(rendered.contains("session.started"));
        assert!(rendered.contains("The repair is complete."));
    }
}
