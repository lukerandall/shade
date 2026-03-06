use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};
use std::io::{self, Stdout};

use crate::vcs::Repo;

pub enum RepoSelectResult {
    Selected(Vec<Repo>),
    Cancelled,
}

struct App {
    repos: Vec<Repo>,
    selected: Vec<bool>,
    cursor: usize,
    filter: String,
    filtered_indices: Vec<usize>,
}

impl App {
    fn new(repos: Vec<Repo>, default_repo: Option<&str>) -> Self {
        let selected: Vec<bool> = repos
            .iter()
            .map(|r| default_repo.is_some_and(|d| r.name == d))
            .collect();
        let filtered_indices = (0..repos.len()).collect();
        Self {
            repos,
            selected,
            cursor: 0,
            filter: String::new(),
            filtered_indices,
        }
    }

    fn apply_filter(&mut self) {
        let query = self.filter.to_lowercase();
        self.filtered_indices = self
            .repos
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                if query.is_empty() {
                    return true;
                }
                r.name.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();

        let max = self.filtered_indices.len().saturating_sub(1);
        if self.cursor > max {
            self.cursor = max;
        }
    }

    fn toggle_current(&mut self) {
        if let Some(&idx) = self.filtered_indices.get(self.cursor) {
            self.selected[idx] = !self.selected[idx];
        }
    }

    fn selected_repos(&self) -> Vec<Repo> {
        self.repos
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i])
            .map(|(_, r)| r.clone())
            .collect()
    }

    fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn move_down(&mut self) {
        let max = self.filtered_indices.len().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        }
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = self.terminal.backend_mut().execute(LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Run the repo multi-select TUI.
/// `default_repo` is the name of the repo to pre-select (e.g. the one the user launched from).
pub fn run_repo_select(
    repos: Vec<Repo>,
    default_repo: Option<&str>,
) -> Result<RepoSelectResult> {
    if repos.is_empty() {
        anyhow::bail!("no repositories found in configured code_dirs");
    }

    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;

    let mut guard = TerminalGuard { terminal };
    let mut app = App::new(repos, default_repo);

    loop {
        guard.terminal.draw(|f| draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(RepoSelectResult::Cancelled);
            }
            match handle_key(&mut app, key) {
                Action::Continue => {}
                Action::Confirm => {
                    let selected = app.selected_repos();
                    if selected.is_empty() {
                        // Don't allow confirming with nothing selected
                        continue;
                    }
                    return Ok(RepoSelectResult::Selected(selected));
                }
                Action::Cancel => return Ok(RepoSelectResult::Cancelled),
            }
        }
    }
}

enum Action {
    Continue,
    Confirm,
    Cancel,
}

fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::Cancel,
        KeyCode::Enter => Action::Confirm,
        KeyCode::Up => {
            app.move_up();
            Action::Continue
        }
        KeyCode::Down => {
            app.move_down();
            Action::Continue
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_up();
            Action::Continue
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_down();
            Action::Continue
        }
        KeyCode::Char(' ') => {
            app.toggle_current();
            Action::Continue
        }
        KeyCode::Tab => {
            app.toggle_current();
            app.move_down();
            Action::Continue
        }
        KeyCode::Backspace => {
            app.filter.pop();
            app.apply_filter();
            Action::Continue
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Toggle all visible
            let all_selected = app
                .filtered_indices
                .iter()
                .all(|&i| app.selected[i]);
            for &i in &app.filtered_indices {
                app.selected[i] = !all_selected;
            }
            Action::Continue
        }
        KeyCode::Char(c) => {
            app.filter.push(c);
            app.apply_filter();
            Action::Continue
        }
        _ => Action::Continue,
    }
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Length(1), // Search
        Constraint::Length(1), // Blank
        Constraint::Min(1),   // List
        Constraint::Length(1), // Help
    ])
    .split(area);

    // Title
    let selected_count = app.selected.iter().filter(|&&s| s).count();
    let title = Line::from(vec![
        Span::styled(
            "Select repos",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({} selected)", selected_count),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(title), chunks[0]);

    // Search
    let search_line = Line::from(vec![
        Span::styled("Search: ", Style::default().fg(Color::Yellow)),
        Span::raw(&app.filter),
    ]);
    f.render_widget(Paragraph::new(search_line), chunks[1]);

    // Cursor position
    let x = chunks[1].x + "Search: ".len() as u16 + app.filter.len() as u16;
    f.set_cursor_position((x, chunks[1].y));

    // Repo list
    draw_list(f, app, chunks[3]);

    // Help
    let help = Paragraph::new(Line::from(Span::styled(
        "Space/Tab: Toggle  Ctrl-A: All  Enter: Confirm  ESC: Cancel",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    )));
    f.render_widget(help, chunks[4]);
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(list_idx, &repo_idx)| {
            let repo = &app.repos[repo_idx];
            let is_selected = app.selected[repo_idx];
            let is_cursor = list_idx == app.cursor;

            let marker = if is_cursor { "> " } else { "  " };
            let checkbox = if is_selected { "[x] " } else { "[ ] " };

            let style = if is_cursor {
                Style::default().fg(Color::Cyan)
            } else if is_selected {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let line = Line::from(vec![
                Span::styled(marker, style),
                Span::styled(checkbox, style),
                Span::styled(&repo.name, style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, area);
}
