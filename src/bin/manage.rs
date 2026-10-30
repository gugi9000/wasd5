use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use serde_json::Value;
use wasd5::{
    db,
    management::{self, Field, Kind, Record, Section},
    pages::Error,
};

#[derive(Parser)]
#[command(about = "Trusted local TUI for wasd5 users, pages, calendar, and more")]
struct Args {
    /// SQLite database path; otherwise DATABASE_URL or wasd5.db
    #[arg(long)]
    database: Option<String>,
    /// Page revision attribution; otherwise WASD_PAGE_EDITOR or an in-app prompt
    #[arg(long)]
    editor_name: Option<String>,
}

struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Screen {
    fn new() -> Result<Self, Error> {
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let mut screen = Self { terminal };
        screen.resume()?;
        Ok(screen)
    }

    fn resume(&mut self) -> Result<(), Error> {
        enable_raw_mode()?;
        execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
        self.terminal.clear()?;
        Ok(())
    }

    fn suspend(&mut self) -> Result<(), Error> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }

    fn edit(&mut self, path: &std::path::Path) -> Result<(), Error> {
        wasd5::editor::command()?;
        self.suspend()?;
        let result = wasd5::editor::run(path);
        self.resume()?;
        result
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if let Err(error) = self.suspend() {
            eprintln!("Failed to restore terminal: {error}");
        }
    }
}

#[derive(Default)]
struct Buffer {
    text: String,
    cursor: usize,
}

impl Buffer {
    fn new(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }

    fn key(&mut self, key: KeyEvent) -> bool {
        let before = self.text.clone();
        match key.code {
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.text.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Backspace => {
                if let Some((previous, _)) = self.text[..self.cursor].char_indices().next_back() {
                    self.text.drain(previous..self.cursor);
                    self.cursor = previous;
                }
            }
            KeyCode::Delete => {
                if let Some(c) = self.text[self.cursor..].chars().next() {
                    self.text.drain(self.cursor..self.cursor + c.len_utf8());
                }
            }
            KeyCode::Left => {
                if let Some((previous, _)) = self.text[..self.cursor].char_indices().next_back() {
                    self.cursor = previous;
                }
            }
            KeyCode::Right => {
                if let Some(c) = self.text[self.cursor..].chars().next() {
                    self.cursor += c.len_utf8();
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            _ => {}
        }
        before != self.text
    }
}

struct Form {
    section: Section,
    original: Option<Record>,
    fields: Vec<(Field, Buffer)>,
    selected: usize,
    dirty: bool,
    files: Vec<tempfile::TempPath>,
}

impl Form {
    fn new(section: Section, original: Option<Record>, attribution: &str) -> Result<Self, Error> {
        if section == Section::Strips && original.is_none() {
            return Err(
                "Upload strip images in the website; use manage to edit existing metadata".into(),
            );
        }
        let mut values = original
            .as_ref()
            .map(|r| r.values.clone())
            .unwrap_or_else(|| management::defaults(section, attribution));
        if section == Section::Pages {
            values["editor"] = Value::String(attribution.into());
        }
        let fields = section
            .fields()
            .into_iter()
            .map(|field| {
                let value = &values[field.key];
                let text = if let Some(text) = value.as_str() {
                    text.into()
                } else if field.kind == Kind::Json {
                    serde_json::to_string_pretty(value)?
                } else {
                    value.to_string()
                };
                Ok((field, Buffer::new(text)))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            section,
            original,
            fields,
            selected: 0,
            dirty: false,
            files: Vec::new(),
        })
    }

    fn values(&self) -> Result<Value, Error> {
        let mut values = serde_json::Map::new();
        for (field, buffer) in &self.fields {
            let value = match field.kind {
                Kind::Boolean => Value::Bool(buffer.text.parse()?),
                Kind::Integer => Value::from(
                    buffer
                        .text
                        .parse::<i32>()
                        .map_err(|_| format!("{} requires a whole number", field.label))?,
                ),
                Kind::Decimal => {
                    let n = buffer
                        .text
                        .parse::<f64>()
                        .map_err(|_| format!("{} requires a number", field.label))?;
                    Value::Number(serde_json::Number::from_f64(n).ok_or("Use a finite number")?)
                }
                Kind::Json => serde_json::from_str(&buffer.text)?,
                _ => Value::String(buffer.text.clone()),
            };
            values.insert(field.key.into(), value);
        }
        Ok(Value::Object(values))
    }

    fn external_edit(
        &mut self,
        edit: &mut impl FnMut(&std::path::Path) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let (field, buffer) = &mut self.fields[self.selected];
        let suffix = if field.kind == Kind::Markdown {
            ".md"
        } else {
            ".json"
        };
        let mut file = tempfile::Builder::new()
            .prefix("wasd5-manage-")
            .suffix(suffix)
            .tempfile()?;
        file.write_all(buffer.text.as_bytes())?;
        file.flush()?;
        let path = file.into_temp_path();
        let result = (|| -> Result<String, Error> {
            edit(&path)?;
            let content = std::fs::read_to_string(&path)?;
            if field.kind == Kind::Json {
                serde_json::from_str::<Value>(&content)?;
            }
            Ok(content)
        })();
        match result {
            Ok(content) => {
                self.dirty |= content != buffer.text;
                *buffer = Buffer::new(content);
                self.files.push(path);
                Ok(())
            }
            Err(error) => {
                let path = path.keep()?;
                Err(format!("{error}. Recovery file kept at {}", path.display()).into())
            }
        }
    }

    fn preserve_files(&mut self) -> Result<Vec<PathBuf>, Error> {
        self.files
            .drain(..)
            .map(|path| path.keep().map_err(Into::into))
            .collect()
    }
}

enum Mode {
    Browse,
    Search(Buffer),
    Form(Form),
    Discard(Form),
    Confirm(Record),
    Details { text: String, scroll: u16 },
    Attribution(Buffer),
}

struct App {
    pool: db::DbPool,
    database: String,
    attribution: String,
    section: usize,
    rows: Vec<Record>,
    selected: usize,
    query: String,
    mode: Mode,
    status: String,
}

impl App {
    fn section(&self) -> Section {
        Section::ALL[self.section]
    }

    fn refresh(&mut self) -> Result<(), Error> {
        self.rows = management::list(&mut *self.pool.get()?, self.section())?;
        self.clamp_selection();
        Ok(())
    }

    fn visible(&self) -> Vec<&Record> {
        let query = self.query.to_lowercase();
        self.rows
            .iter()
            .filter(|row| {
                query.is_empty()
                    || format!("{} {}", row.id, row.summary)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect()
    }

    fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.visible().len().saturating_sub(1));
    }

    fn selected(&self) -> Option<Record> {
        self.visible().get(self.selected).map(|r| (*r).clone())
    }

    fn switch(&mut self, direction: isize) -> Result<(), Error> {
        self.section =
            (self.section as isize + direction).rem_euclid(Section::ALL.len() as isize) as usize;
        self.selected = 0;
        self.query.clear();
        self.rows.clear();
        self.refresh()
    }

    fn handle(
        &mut self,
        key: KeyEvent,
        edit: &mut impl FnMut(&std::path::Path) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let mode = std::mem::replace(&mut self.mode, Mode::Browse);
        match mode {
            Mode::Attribution(mut buffer) => {
                if key.code == KeyCode::Esc {
                    return Ok(true);
                }
                if key.code == KeyCode::Enter && !buffer.text.trim().is_empty() {
                    self.attribution = buffer.text.trim().to_owned();
                    self.status = "Ready. Left/Right switches sections; n creates; e edits.".into();
                } else {
                    buffer.key(key);
                    self.mode = Mode::Attribution(buffer);
                }
            }
            Mode::Browse => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(true);
                }
                KeyCode::Tab | KeyCode::Right => self.switch(1)?,
                KeyCode::BackTab | KeyCode::Left => self.switch(-1)?,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected += 1;
                    self.clamp_selection();
                }
                KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
                KeyCode::Home => self.selected = 0,
                KeyCode::End => self.selected = self.visible().len().saturating_sub(1),
                KeyCode::Char('r') => {
                    self.refresh()?;
                    self.status = "Reloaded from SQL.".into();
                }
                KeyCode::Char('/') => self.mode = Mode::Search(Buffer::new(self.query.clone())),
                KeyCode::Char('n') => {
                    self.mode = Mode::Form(Form::new(self.section(), None, &self.attribution)?)
                }
                KeyCode::Char('e') | KeyCode::Enter => {
                    if let Some(record) = self.selected() {
                        self.mode =
                            Mode::Form(Form::new(self.section(), Some(record), &self.attribution)?);
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(record) = self.selected() {
                        self.mode = Mode::Confirm(record);
                    }
                }
                KeyCode::Char('v') => {
                    if let Some(record) = self.selected() {
                        let text =
                            management::details(&mut *self.pool.get()?, self.section(), &record)?;
                        self.mode = Mode::Details { text, scroll: 0 };
                    }
                }
                _ => {}
            },
            Mode::Search(mut buffer) => {
                match key.code {
                    KeyCode::Esc => {
                        self.query.clear();
                        self.selected = 0;
                    }
                    KeyCode::Enter => {}
                    _ => {
                        buffer.key(key);
                        self.query = buffer.text.clone();
                        self.selected = 0;
                        self.mode = Mode::Search(buffer);
                    }
                }
                self.clamp_selection();
            }
            Mode::Form(mut form) => {
                if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    let result = form.values().and_then(|values| {
                        management::save(
                            &mut *self.pool.get()?,
                            form.section,
                            form.original.as_ref(),
                            values,
                        )
                    });
                    match result {
                        Ok(()) => {
                            self.refresh()?;
                            self.status = "Saved to SQL.".into();
                        }
                        Err(error) => {
                            let recovery = match form.preserve_files() {
                                Ok(paths) if !paths.is_empty() => format!(
                                    " | Recovery: {}",
                                    paths
                                        .iter()
                                        .map(|p| p.display().to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                                Ok(_) => String::new(),
                                Err(error) => {
                                    format!(" | Could not preserve recovery files: {error}")
                                }
                            };
                            self.status = format!("Save failed: {error}{recovery}");
                            self.mode = Mode::Form(form);
                        }
                    }
                } else if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    self.mode = if form.dirty {
                        Mode::Discard(form)
                    } else {
                        Mode::Browse
                    };
                } else {
                    match key.code {
                        KeyCode::Tab | KeyCode::Down => {
                            form.selected = (form.selected + 1) % form.fields.len()
                        }
                        KeyCode::BackTab | KeyCode::Up => {
                            form.selected =
                                (form.selected + form.fields.len() - 1) % form.fields.len()
                        }
                        KeyCode::Enter => match form.fields[form.selected].0.kind {
                            Kind::Markdown | Kind::Json => {
                                if let Err(error) = form.external_edit(edit) {
                                    self.status = error.to_string();
                                } else {
                                    self.status =
                                        "Editor returned. Ctrl+S saves to SQL; Esc cancels.".into();
                                }
                            }
                            Kind::Boolean => {
                                let buffer = &mut form.fields[form.selected].1;
                                buffer.text = (buffer.text != "true").to_string();
                                form.dirty = true;
                            }
                            _ => form.selected = (form.selected + 1) % form.fields.len(),
                        },
                        _ => {
                            let (field, buffer) = &mut form.fields[form.selected];
                            if field.kind == Kind::Boolean && key.code == KeyCode::Char(' ') {
                                buffer.text = (buffer.text != "true").to_string();
                                form.dirty = true;
                            } else if !matches!(
                                field.kind,
                                Kind::Boolean | Kind::Markdown | Kind::Json
                            ) {
                                form.dirty |= buffer.key(key);
                            }
                        }
                    }
                    self.mode = Mode::Form(form);
                }
            }
            Mode::Discard(form) => {
                if key.code == KeyCode::Char('y') {
                    self.status = "Draft discarded. SQL was not changed.".into();
                } else if matches!(key.code, KeyCode::Char('n') | KeyCode::Esc) {
                    self.mode = Mode::Form(form);
                } else {
                    self.mode = Mode::Discard(form);
                }
            }
            Mode::Confirm(record) => {
                if key.code == KeyCode::Char('y') {
                    management::remove(
                        &mut *self.pool.get()?,
                        self.section(),
                        &record,
                        &self.attribution,
                    )?;
                    self.refresh()?;
                    self.status = "Action completed.".into();
                } else if !matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) {
                    self.mode = Mode::Confirm(record);
                }
            }
            Mode::Details { text, mut scroll } => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(false),
                    KeyCode::Down | KeyCode::Char('j') => scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => scroll = scroll.saturating_sub(1),
                    KeyCode::PageDown => scroll = scroll.saturating_add(10),
                    KeyCode::PageUp => scroll = scroll.saturating_sub(10),
                    KeyCode::Home => scroll = 0,
                    _ => {}
                }
                self.mode = Mode::Details { text, scroll };
            }
        }
        Ok(false)
    }

    fn draw(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.width < 45 || area.height < 12 {
            frame.render_widget(
                Paragraph::new("Resize terminal to at least 45 columns x 12 rows.")
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(4),
                Constraint::Length(4),
            ])
            .split(area);
        frame.render_widget(
            Paragraph::new(safe(&format!(
                "manage | {} | attribution: {}",
                self.database, self.attribution
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("wasd5 - trusted local administration"),
            ),
            layout[0],
        );
        let highlight = Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        match &self.mode {
            Mode::Form(form) | Mode::Discard(form) => {
                let items: Vec<_> = form
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(index, (field, buffer))| {
                        let value = match field.kind {
                            Kind::Password => "*".repeat(buffer.text.chars().count()),
                            Kind::Boolean => {
                                if buffer.text == "true" {
                                    "[x] true".into()
                                } else {
                                    "[ ] false".into()
                                }
                            }
                            Kind::Markdown | Kind::Json => format!(
                                "{} [Enter to edit]",
                                buffer.text.lines().next().unwrap_or("")
                            ),
                            _ => {
                                let mut value = buffer.text.clone();
                                if index == form.selected {
                                    value.insert(buffer.cursor.min(value.len()), '|');
                                }
                                value
                            }
                        };
                        ListItem::new(format!("{}\n  {}\n", field.label, safe(&value)))
                    })
                    .collect();
                let title = format!(
                    "{} {} | Tab/Up/Down: field | Enter: toggle/editor | Ctrl+S: save | Esc: cancel",
                    if form.original.is_some() {
                        "Edit"
                    } else {
                        "Create"
                    },
                    form.section.title()
                );
                let list = List::new(items)
                    .highlight_style(highlight)
                    .block(Block::default().borders(Borders::ALL).title(title));
                let mut state = ListState::default().with_selected(Some(form.selected));
                frame.render_stateful_widget(list, layout[1], &mut state);
            }
            Mode::Details { text, scroll } => {
                frame.render_widget(
                    Paragraph::new(safe(text))
                        .wrap(Wrap { trim: false })
                        .scroll((*scroll, 0))
                        .block(Block::default().borders(Borders::ALL).title(
                            "Details / page revision history | Up/Down/PgUp/PgDn | Esc: close",
                        )),
                    layout[1],
                );
            }
            Mode::Attribution(buffer) => {
                frame.render_widget(Paragraph::new(format!("Page edits require attribution.\nEnter your editing name (or set WASD_PAGE_EDITOR / --editor-name).\n\n{}|\n\nEnter: continue | Esc: quit", safe(&buffer.text)))
                    .block(Block::default().borders(Borders::ALL).title("Editor attribution")), layout[1]);
            }
            _ => {
                let columns = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(if area.width >= 90 { 28 } else { 20 }),
                        Constraint::Min(20),
                    ])
                    .split(layout[1]);
                let nav = List::new(Section::ALL.iter().map(|s| ListItem::new(s.title())))
                    .highlight_style(highlight)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Left/Right or Tab"),
                    );
                frame.render_stateful_widget(
                    nav,
                    columns[0],
                    &mut ListState::default().with_selected(Some(self.section)),
                );
                let rows = self.visible();
                let title = format!(
                    "{} ({} records) | filter: {}",
                    self.section().title(),
                    rows.len(),
                    safe(&self.query)
                );
                let list = List::new(
                    rows.iter()
                        .map(|r| ListItem::new(safe(&format!("#{} {}", r.id, r.summary)))),
                )
                .highlight_style(highlight)
                .block(Block::default().borders(Borders::ALL).title(title));
                let selection = (!rows.is_empty()).then_some(self.selected);
                frame.render_stateful_widget(
                    list,
                    columns[1],
                    &mut ListState::default().with_selected(selection),
                );
            }
        }
        let footer = match &self.mode {
            Mode::Confirm(record) => format!(
                "{}: #{} {}? y: confirm / n: cancel",
                self.section().delete_label(),
                record.id,
                record.summary
            ),
            Mode::Discard(_) => "Discard unsaved draft? y: discard / n: keep editing".into(),
            Mode::Search(buffer) => format!("Filter: {}|  Enter: apply / Esc: clear", buffer.text),
            Mode::Browse => format!(
                "n: new  e/Enter: edit  d: {}  v: details/history  /: filter  r: reload  q: quit\n{}",
                self.section().delete_label(),
                self.status
            ),
            _ => self.status.clone(),
        };
        frame.render_widget(
            Paragraph::new(safe(&footer))
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL)),
            layout[2],
        );
    }
}

fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("manage requires an interactive terminal. Use cli or pages for non-interactive commands.".into());
    }
    let database = args
        .database
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "wasd5.db".into());
    let pool = db::establish_pool(&database);
    db::run_migrations(&mut *pool.get()?)?;
    let attribution = args
        .editor_name
        .or_else(|| std::env::var("WASD_PAGE_EDITOR").ok())
        .unwrap_or_default()
        .trim()
        .to_owned();
    let mut app = App {
        pool,
        database,
        mode: if attribution.is_empty() {
            Mode::Attribution(Buffer::default())
        } else {
            Mode::Browse
        },
        attribution,
        section: 0,
        rows: Vec::new(),
        selected: 0,
        query: String::new(),
        status: "Left/Right switches sections. Users are disabled, never permanently deleted."
            .into(),
    };
    app.refresh()?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        previous_hook(info);
    }));
    let mut screen = Screen::new()?;
    loop {
        screen.terminal.draw(|frame| app.draw(frame))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.handle(key, &mut |path| screen.edit(path)) {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(error) => app.status = format!("Error: {error}"),
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn unicode_input_and_password_masking() {
        let mut buffer = Buffer::new("ab".into());
        buffer.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        buffer.key(KeyEvent::new(KeyCode::Char('\u{00e6}'), KeyModifiers::NONE));
        assert_eq!(buffer.text, "a\u{00e6}b");
        buffer.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(buffer.text, "ab");
        let mut form = Form::new(Section::Users, None, "tester").unwrap();
        form.fields
            .iter_mut()
            .find(|(f, _)| f.kind == Kind::Password)
            .unwrap()
            .1 = Buffer::new("SECRET".into());
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("test.db").to_str().unwrap());
        db::run_migrations(&mut pool.get().unwrap()).unwrap();
        let app = App {
            pool,
            database: "test".into(),
            attribution: "tester".into(),
            section: 0,
            rows: Vec::new(),
            selected: 0,
            query: String::new(),
            mode: Mode::Form(form),
            status: String::new(),
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!rendered.contains("SECRET"));
        assert!(rendered.contains("******"));
    }

    #[test]
    fn all_sections_render_and_filter_without_a_real_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("test.db").to_str().unwrap());
        db::run_migrations(&mut pool.get().unwrap()).unwrap();
        let mut app = App {
            pool,
            database: "test".into(),
            attribution: "tester".into(),
            section: 0,
            rows: Vec::new(),
            selected: 0,
            query: String::new(),
            mode: Mode::Browse,
            status: String::new(),
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        for _ in Section::ALL {
            app.refresh().unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            if app.section() != Section::Strips {
                app.mode = Mode::Form(Form::new(app.section(), None, "tester").unwrap());
                terminal.draw(|frame| app.draw(frame)).unwrap();
                app.mode = Mode::Browse;
            }
            app.switch(1).unwrap();
        }
        app.section = 3;
        app.refresh().unwrap();
        assert!(!app.visible().is_empty());
        app.query = "no-such-person".into();
        assert!(app.visible().is_empty());
        app.clamp_selection();
        assert_eq!(app.selected, 0);
    }

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("test.db").to_str().unwrap());
        db::run_migrations(&mut pool.get().unwrap()).unwrap();
        (
            App {
                pool,
                database: "test".into(),
                attribution: "Operator".into(),
                section: 1,
                rows: Vec::new(),
                selected: 0,
                query: String::new(),
                mode: Mode::Browse,
                status: String::new(),
            },
            dir,
        )
    }

    #[test]
    fn editor_handoff_save_and_page_history_through_ui_keys() {
        let (mut app, _dir) = app();
        let mut editor = |path: &std::path::Path| -> Result<(), Error> {
            std::fs::write(path, "# Edited in system editor\n")?;
            Ok(())
        };
        app.handle(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        let Mode::Form(form) = &mut app.mode else {
            panic!("Expected form");
        };
        form.fields
            .iter_mut()
            .find(|(f, _)| f.key == "slug")
            .unwrap()
            .1 = Buffer::new("tui/page".into());
        form.selected = form
            .fields
            .iter()
            .position(|(f, _)| f.kind == Kind::Markdown)
            .unwrap();
        app.handle(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        let Mode::Form(form) = &app.mode else {
            panic!("Expected form");
        };
        let temp = form.files[0].to_path_buf();
        assert!(temp.exists());
        assert_eq!(
            wasd5::pages::list(&mut app.pool.get().unwrap(), true)
                .unwrap()
                .len(),
            0
        );
        app.handle(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &mut editor,
        )
        .unwrap();
        assert!(matches!(app.mode, Mode::Browse));
        assert!(!temp.exists());
        let page = wasd5::pages::list(&mut app.pool.get().unwrap(), true)
            .unwrap()
            .remove(0);
        assert_eq!(page.content, "# Edited in system editor\n");
        assert_eq!(page.creator, "Operator");
        app.handle(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        let Mode::Details { text, .. } = &app.mode else {
            panic!("Expected history");
        };
        assert!(text.contains("revisions"));
        assert!(text.contains("Operator"));
    }

    #[test]
    fn conflict_retains_draft_recovery_file_and_cancel_requires_confirmation() {
        let (mut app, _dir) = app();
        let page = wasd5::pages::create(
            &mut app.pool.get().unwrap(),
            "conflict",
            "Original",
            "Operator",
            true,
        )
        .unwrap();
        app.refresh().unwrap();
        let mut editor = |path: &std::path::Path| -> Result<(), Error> {
            std::fs::write(path, "My unsaved edit")?;
            Ok(())
        };
        app.handle(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        let Mode::Form(form) = &mut app.mode else {
            panic!("Expected form");
        };
        form.selected = form
            .fields
            .iter()
            .position(|(f, _)| f.kind == Kind::Markdown)
            .unwrap();
        app.handle(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        let Mode::Form(form) = &app.mode else {
            panic!("Expected form");
        };
        let temp = form.files[0].to_path_buf();
        {
            let mut conn = app.pool.get().unwrap();
            let revision = wasd5::pages::latest_revision(&mut conn, page.id).unwrap();
            wasd5::pages::edit(
                &mut conn,
                page.id,
                revision,
                "Other edit",
                true,
                "Other editor",
            )
            .unwrap();
        }
        app.handle(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &mut editor,
        )
        .unwrap();
        assert!(matches!(app.mode, Mode::Form(_)));
        assert!(app.status.contains("Record changed"));
        assert!(app.status.contains("Recovery:"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "My unsaved edit");
        app.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut editor)
            .unwrap();
        assert!(matches!(app.mode, Mode::Discard(_)));
        app.handle(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        assert!(matches!(app.mode, Mode::Form(_)));
        app.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut editor)
            .unwrap();
        app.handle(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut editor,
        )
        .unwrap();
        assert!(matches!(app.mode, Mode::Browse));
        assert_eq!(
            wasd5::pages::by_id(&mut app.pool.get().unwrap(), page.id)
                .unwrap()
                .content,
            "Other edit"
        );
        std::fs::remove_file(temp).unwrap();
    }

    #[test]
    fn failed_editor_preserves_file_without_mutating_the_draft() {
        let mut form = Form::new(Section::Pages, None, "Operator").unwrap();
        form.selected = form
            .fields
            .iter()
            .position(|(f, _)| f.kind == Kind::Markdown)
            .unwrap();
        let mut temp = PathBuf::new();
        let error = form
            .external_edit(&mut |path| {
                temp = path.to_path_buf();
                std::fs::write(path, "Recovery content")?;
                Err("Editor failed".into())
            })
            .unwrap_err();
        assert!(error.to_string().contains("Recovery file kept at"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "Recovery content");
        assert_eq!(form.fields[form.selected].1.text, "# New page\n");
        std::fs::remove_file(temp).unwrap();
    }
}
