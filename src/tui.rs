use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use jiff::civil::Date;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};
use std::io::{self, Stdout};

use crate::config::Config;
use crate::env::{self, Environment};
use crate::slug;

/// The result of running the interactive TUI.
pub enum TuiResult {
    /// User selected an existing environment.
    Selected(Environment),
    /// User wants to create a new environment (returns the slugified label).
    Create(String),
    /// User pressed Esc to cancel.
    Cancelled,
}

/// The current mode of the TUI.
enum Mode {
    /// Browsing / filtering the environment list.
    Browse,
    /// Entering a name for a new environment.
    CreateInput,
    /// Confirming deletion of an environment.
    DeleteConfirm(usize),
}

struct App {
    environments: Vec<Environment>,
    filtered_indices: Vec<usize>,
    filter: String,
    cursor: usize,
    mode: Mode,
    create_input: String,
    env_dir: String,
    today: Date,
}

impl App {
    fn new(environments: Vec<Environment>, env_dir: String) -> Self {
        let filtered_indices: Vec<usize> = (0..environments.len()).collect();
        Self {
            environments,
            filtered_indices,
            filter: String::new(),
            cursor: 0,
            mode: Mode::Browse,
            create_input: String::new(),
            env_dir,
            today: jiff::Zoned::now().date(),
        }
    }

    fn apply_filter(&mut self) {
        let query = self.filter.to_lowercase();
        self.filtered_indices = self
            .environments
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if query.is_empty() {
                    return true;
                }
                e.label.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();

        // Keep cursor in bounds; the +1 accounts for "+ Create new"
        let max = self.total_items().saturating_sub(1);
        if self.cursor > max {
            self.cursor = max;
        }
    }

    /// Total visible items = filtered envs + 1 for "+ Create new".
    fn total_items(&self) -> usize {
        self.filtered_indices.len() + 1
    }

    fn is_create_selected(&self) -> bool {
        self.cursor == self.filtered_indices.len()
    }

    fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn move_down(&mut self) {
        let max = self.total_items().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    fn refresh_environments(&mut self) -> Result<()> {
        self.environments = env::list_environments(&self.env_dir)?;
        self.apply_filter();
        Ok(())
    }
}

/// Format a date as a relative time string compared to today.
fn format_relative_date(date: Date, today: Date) -> String {
    let diff = today.since(date);
    let days = match diff {
        Ok(span) => span.get_days(),
        Err(_) => return date.to_string(),
    };

    if days < 0 {
        return format!("in {}d", -days);
    }

    match days {
        0 => "today".to_string(),
        1 => "1d ago".to_string(),
        2..=6 => format!("{}d ago", days),
        7..=13 => "1w ago".to_string(),
        14..=20 => "2w ago".to_string(),
        21..=27 => "3w ago".to_string(),
        28..=44 => "1mo ago".to_string(),
        45..=75 => "2mo ago".to_string(),
        76..=105 => "3mo ago".to_string(),
        106..=135 => "4mo ago".to_string(),
        136..=165 => "5mo ago".to_string(),
        166..=195 => "6mo ago".to_string(),
        196..=345 => format!("{}mo ago", (days + 15) / 30),
        346..=729 => "1y ago".to_string(),
        _ => format!("{}y ago", days / 365),
    }
}

/// A drop guard that restores the terminal when dropped.
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

/// Run the interactive TUI and return the user's choice.
pub fn run_tui(config: &Config) -> Result<TuiResult> {
    let environments = env::list_environments(&config.env_dir)?;

    // Set up terminal
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;

    let mut guard = TerminalGuard { terminal };
    let mut app = App::new(environments, config.env_dir.clone());

    loop {
        guard.terminal.draw(|f| draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(TuiResult::Cancelled);
            }
            match &app.mode {
                Mode::Browse => match handle_browse_key(&mut app, key) {
                    BrowseAction::Continue => {}
                    BrowseAction::SelectEnv(idx) => {
                        let env = app.environments[idx].clone();
                        return Ok(TuiResult::Selected(env));
                    }
                    BrowseAction::CreateWithName(label) => {
                        return Ok(TuiResult::Create(label));
                    }
                    BrowseAction::EnterCreate => {
                        app.mode = Mode::CreateInput;
                        app.create_input.clear();
                    }
                    BrowseAction::Cancel => {
                        return Ok(TuiResult::Cancelled);
                    }
                },
                Mode::CreateInput => match handle_create_key(&mut app, key) {
                    CreateAction::Continue => {}
                    CreateAction::Submit(label) => {
                        return Ok(TuiResult::Create(label));
                    }
                    CreateAction::Cancel => {
                        app.mode = Mode::Browse;
                    }
                },
                Mode::DeleteConfirm(_) => {
                    // Copy the index out before borrowing app mutably
                    let idx = match app.mode {
                        Mode::DeleteConfirm(i) => i,
                        _ => unreachable!(),
                    };
                    match handle_delete_key(&mut app, key, idx)? {
                        DeleteAction::Continue => {}
                        DeleteAction::Done => {}
                    }
                }
            }
        }
    }
}

enum BrowseAction {
    Continue,
    SelectEnv(usize),
    EnterCreate,
    CreateWithName(String),
    Cancel,
}

fn handle_browse_key(app: &mut App, key: KeyEvent) -> BrowseAction {
    match key.code {
        KeyCode::Esc => BrowseAction::Cancel,
        KeyCode::Up => {
            app.move_up();
            BrowseAction::Continue
        }
        KeyCode::Down => {
            app.move_down();
            BrowseAction::Continue
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_up();
            BrowseAction::Continue
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_down();
            BrowseAction::Continue
        }
        KeyCode::Enter => {
            if app.is_create_selected() {
                // If there's already text in the filter, use it as the name
                let slugified = slug::slugify(&app.filter);
                if slug::validate_slug(&slugified).is_ok() {
                    BrowseAction::CreateWithName(slugified)
                } else {
                    BrowseAction::EnterCreate
                }
            } else if let Some(&idx) = app.filtered_indices.get(app.cursor) {
                BrowseAction::SelectEnv(idx)
            } else {
                BrowseAction::Continue
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !app.is_create_selected()
                && let Some(&idx) = app.filtered_indices.get(app.cursor)
            {
                app.mode = Mode::DeleteConfirm(idx);
            }
            BrowseAction::Continue
        }
        KeyCode::Backspace => {
            app.filter.pop();
            app.apply_filter();
            BrowseAction::Continue
        }
        KeyCode::Char(c) => {
            app.filter.push(c);
            app.apply_filter();
            BrowseAction::Continue
        }
        _ => BrowseAction::Continue,
    }
}

enum CreateAction {
    Continue,
    Submit(String),
    Cancel,
}

fn handle_create_key(app: &mut App, key: KeyEvent) -> CreateAction {
    match key.code {
        KeyCode::Esc => CreateAction::Cancel,
        KeyCode::Enter => {
            let slugified = slug::slugify(&app.create_input);
            match slug::validate_slug(&slugified) {
                Ok(()) => CreateAction::Submit(slugified),
                Err(_) => CreateAction::Continue,
            }
        }
        KeyCode::Backspace => {
            app.create_input.pop();
            CreateAction::Continue
        }
        KeyCode::Char(c) => {
            app.create_input.push(c);
            CreateAction::Continue
        }
        _ => CreateAction::Continue,
    }
}

enum DeleteAction {
    Continue,
    Done,
}

fn handle_delete_key(app: &mut App, key: KeyEvent, env_idx: usize) -> Result<DeleteAction> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let env = &app.environments[env_idx];
            env::delete_environment(env)?;
            app.mode = Mode::Browse;
            app.refresh_environments()?;
            Ok(DeleteAction::Done)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = Mode::Browse;
            Ok(DeleteAction::Done)
        }
        _ => Ok(DeleteAction::Continue),
    }
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Length(1), // Search bar
        Constraint::Length(1), // Blank line
        Constraint::Min(1),    // List
        Constraint::Length(1), // Help bar
    ])
    .split(area);

    draw_title(f, chunks[0]);
    draw_search(f, app, chunks[1]);
    draw_list(f, app, chunks[3]);
    draw_help(f, app, chunks[4]);
}

fn draw_title(f: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(Span::styled(
        "Shade",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    f.render_widget(title, area);
}

fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    let (label_text, input_text) = match &app.mode {
        Mode::CreateInput => ("Name: ", app.create_input.as_str()),
        _ => ("Search: ", app.filter.as_str()),
    };

    let line = Line::from(vec![
        Span::styled(label_text, Style::default().fg(Color::Yellow)),
        Span::raw(input_text),
    ]);
    let search = Paragraph::new(line);
    f.render_widget(search, area);

    // Position cursor after the input text
    if matches!(app.mode, Mode::Browse | Mode::CreateInput) {
        let x = area.x + label_text.len() as u16 + input_text.len() as u16;
        let y = area.y;
        f.set_cursor_position((x, y));
    }
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    match &app.mode {
        Mode::DeleteConfirm(idx) => {
            let name = &app.environments[*idx].name;
            let line = Line::from(vec![
                Span::styled("Delete ", Style::default().fg(Color::Red)),
                Span::styled(
                    name.as_str(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("? (y/n)", Style::default().fg(Color::Red)),
            ]);
            f.render_widget(Paragraph::new(line), area);
        }
        Mode::CreateInput | Mode::Browse => {
            render_env_list(f, app, area);
        }
    }
}

fn render_env_list(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    for (list_idx, &env_idx) in app.filtered_indices.iter().enumerate() {
        let env = &app.environments[env_idx];
        let selected = list_idx == app.cursor && !matches!(app.mode, Mode::CreateInput);

        let marker = if selected { "> " } else { "  " };
        let age = format_relative_date(env.date, app.today);

        // Calculate padding for right-aligned age
        let name_len = marker.len() + env.name.len();
        let available = area.width as usize;
        let age_len = age.len();
        let padding = if available > name_len + age_len + 2 {
            available - name_len - age_len
        } else {
            2
        };

        // Split the env name into date and label parts
        let date_part = &env.name[..10];
        let label_part = &env.name[10..]; // includes leading dash

        let style_modifier = if selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        let line = Line::from(vec![
            Span::styled(marker, style_modifier),
            Span::styled(
                date_part,
                Style::default().fg(Color::DarkGray).patch(style_modifier),
            ),
            Span::styled(
                label_part,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
                    .patch(style_modifier),
            ),
            Span::raw(" ".repeat(padding)),
            Span::styled(age, Style::default().fg(Color::DarkGray)),
        ]);

        items.push(ListItem::new(line));
    }

    // "+ Create new" at the bottom
    let create_selected =
        app.cursor == app.filtered_indices.len() && !matches!(app.mode, Mode::CreateInput);
    let create_marker = if create_selected { "> " } else { "  " };
    let create_style = if create_selected {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Green)
    };

    // Add a blank separator line before "+ Create new" if there are environments
    if !app.filtered_indices.is_empty() {
        items.push(ListItem::new(Line::raw("")));
    }

    let create_label = if !app.filter.is_empty() {
        let slugified = slug::slugify(&app.filter);
        if slugified.is_empty() {
            "+ Create new".to_string()
        } else {
            format!("+ Create new: {}", slugified)
        }
    } else {
        "+ Create new".to_string()
    };

    items.push(ListItem::new(Line::from(vec![
        Span::styled(create_marker, create_style),
        Span::styled(create_label, create_style),
    ])));

    let list = List::new(items);
    f.render_widget(list, area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let help_text = match &app.mode {
        Mode::Browse => "Up/Down: Navigate  Enter: Select  Ctrl-D: Delete  ESC: Cancel",
        Mode::CreateInput => "Enter: Create  ESC: Back",
        Mode::DeleteConfirm(_) => "y: Confirm  n/ESC: Cancel",
    };

    let help = Paragraph::new(Line::from(Span::styled(
        help_text,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    )));
    f.render_widget(help, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_relative_date_today() {
        let today: Date = "2026-03-05".parse().unwrap();
        assert_eq!(format_relative_date(today, today), "today");
    }

    #[test]
    fn test_format_relative_date_yesterday() {
        let today: Date = "2026-03-05".parse().unwrap();
        let yesterday: Date = "2026-03-04".parse().unwrap();
        assert_eq!(format_relative_date(yesterday, today), "1d ago");
    }

    #[test]
    fn test_format_relative_date_days() {
        let today: Date = "2026-03-05".parse().unwrap();
        let three_days: Date = "2026-03-02".parse().unwrap();
        assert_eq!(format_relative_date(three_days, today), "3d ago");
    }

    #[test]
    fn test_format_relative_date_week() {
        let today: Date = "2026-03-05".parse().unwrap();
        let week_ago: Date = "2026-02-26".parse().unwrap();
        assert_eq!(format_relative_date(week_ago, today), "1w ago");
    }

    #[test]
    fn test_format_relative_date_month() {
        let today: Date = "2026-03-05".parse().unwrap();
        let month_ago: Date = "2026-02-01".parse().unwrap();
        assert_eq!(format_relative_date(month_ago, today), "1mo ago");
    }
}
