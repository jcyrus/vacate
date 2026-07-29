//! Interactive mode: the state machine and the event loop.
//!
//! Rendering lives in [`crate::ui`]. Nothing in `App` touches the terminal, so
//! the whole interaction model is unit-testable by feeding it key events.

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::fuzzy;
use crate::kill::{self, Signal};
use crate::ports::{self, PortEntry};
use crate::ui;

pub enum Status {
    Info(String),
    Error(String),
}

pub struct App {
    /// Everything we found on the last refresh.
    entries: Vec<PortEntry>,
    /// Indices into `entries` that survive the current query, best match first.
    visible: Vec<usize>,
    /// Index into `visible`, not into `entries`.
    selected: usize,
    query: String,
    searching: bool,
    status: Option<Status>,
    quit: bool,
}

impl App {
    pub fn new(entries: Vec<PortEntry>) -> Self {
        let mut app = Self {
            entries,
            visible: Vec::new(),
            selected: 0,
            query: String::new(),
            searching: false,
            status: None,
            quit: false,
        };
        app.apply_filter();
        app
    }

    pub fn visible(&self) -> impl Iterator<Item = &PortEntry> {
        self.visible.iter().map(|&i| &self.entries[i])
    }

    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn selected(&self) -> Option<usize> {
        (!self.visible.is_empty()).then_some(self.selected)
    }

    pub fn selected_entry(&self) -> Option<&PortEntry> {
        self.visible.get(self.selected).map(|&i| &self.entries[i])
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn searching(&self) -> bool {
        self.searching
    }

    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Re-rank `visible` against the current query, keeping the highlighted
    /// row on the same process where possible.
    fn apply_filter(&mut self) {
        let anchor = self.selected_entry().map(|e| (e.port, e.pid));

        let mut scored: Vec<(i32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                fuzzy::score(&entry.haystack(), &self.query).map(|score| (score, i))
            })
            .collect();
        // Best score first; ties keep the port-then-pid order from the scan so
        // the list doesn't shuffle under the cursor.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.visible = scored.into_iter().map(|(_, i)| i).collect();

        self.selected = anchor
            .and_then(|key| {
                self.visible
                    .iter()
                    .position(|&i| (self.entries[i].port, self.entries[i].pid) == key)
            })
            .unwrap_or(0)
            .min(self.visible.len().saturating_sub(1));
    }

    pub fn refresh(&mut self) {
        match ports::scan() {
            Ok(entries) => {
                self.entries = entries;
                self.apply_filter();
            }
            Err(err) => self.status = Some(Status::Error(format!("Refresh failed: {err}"))),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    fn kill_selected(&mut self, signal: Signal) {
        let Some(entry) = self.selected_entry() else {
            self.status = Some(Status::Error("Nothing selected.".into()));
            return;
        };
        let (pid, name, port) = (entry.pid, entry.name.clone(), entry.port);

        self.status = Some(match kill::send(pid, signal) {
            Ok(()) => Status::Info(format!("Sent {signal} to {name} (PID {pid}) on :{port}")),
            Err(err) => Status::Error(err.to_string()),
        });

        // The process usually needs a moment to actually exit, so the row may
        // still be here after this refresh. That's honest — it's what the OS
        // reports, and `r` will confirm once it's really gone.
        self.refresh();
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Terminals in kitty/Windows modes also emit release events; acting on
        // both would double every keystroke.
        if key.kind != KeyEventKind::Press {
            return;
        }
        // A new keystroke means the last result has been read.
        self.status = None;

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => self.quit = true,
                KeyCode::Char('d') => self.move_selection(10),
                KeyCode::Char('u') => self.move_selection(-10),
                _ => {}
            }
            return;
        }

        if self.searching {
            self.handle_search_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.selected = self.visible.len().saturating_sub(1);
            }
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Enter => self.kill_selected(Signal::Term),
            // Shift+K, distinct from lowercase `k`, which is vim's "up".
            KeyCode::Char('K') => self.kill_selected(Signal::Kill),
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            // Esc abandons the search; Enter keeps the filter and hands the
            // keyboard back to the normal-mode bindings.
            KeyCode::Esc => {
                self.searching = false;
                self.query.clear();
                self.apply_filter();
            }
            KeyCode::Enter => self.searching = false,
            KeyCode::Backspace => {
                self.query.pop();
                self.apply_filter();
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.apply_filter();
            }
            // Let the user aim while still typing.
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            _ => {}
        }
    }
}

/// Take over the terminal and run until the user quits.
pub fn run() -> Result<()> {
    let entries = ports::scan()?;
    // `init` installs a panic hook that puts the terminal back, so a crash
    // can't leave the user in a broken shell.
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, App::new(entries));
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, mut app: App) -> Result<()> {
    while !app.quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        // Blocking read: idle costs exactly zero CPU, which is the whole point
        // of not having a polling tick here.
        if let Event::Key(key) = event::read()? {
            app.handle_key(key);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Proto;

    fn entry(port: u16, pid: u32, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Proto::Tcp,
            pid,
            name: name.to_owned(),
            user: "cyrus".to_owned(),
            memory: 1024,
        }
    }

    fn app() -> App {
        App::new(vec![
            entry(3000, 100, "node"),
            entry(5432, 200, "postgres"),
            entry(8080, 300, "python"),
        ])
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn type_query(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn starts_with_everything_visible() {
        let app = app();
        assert_eq!(app.visible_count(), 3);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.selected_entry().unwrap().port, 3000);
    }

    #[test]
    fn j_and_k_navigate_without_running_off_the_ends() {
        let mut app = app();
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected(), Some(0), "already at the top");

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selected(), Some(2), "clamped at the bottom");

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected(), Some(1));
    }

    #[test]
    fn g_jumps_to_the_ends() {
        let mut app = app();
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.selected(), Some(2));
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn slash_enters_search_and_filters_by_name() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        assert!(app.searching());

        type_query(&mut app, "post");
        // Fuzzy matching is a subsequence match, so loose hits can survive;
        // what matters is that the obvious one is ranked first and selected.
        assert!(app.visible_count() < 3, "the filter should narrow the list");
        assert_eq!(app.selected_entry().unwrap().name, "postgres");
        assert_eq!(app.visible().next().unwrap().name, "postgres");
        assert_eq!(app.total_count(), 3, "filtering must not drop the data");
    }

    #[test]
    fn search_matches_port_numbers_too() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        type_query(&mut app, "8080");
        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_entry().unwrap().port, 8080);
    }

    #[test]
    fn backspace_widens_the_filter_again() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        type_query(&mut app, "post");
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.query(), "");
        assert_eq!(app.visible_count(), 3);
    }

    #[test]
    fn in_search_mode_letters_type_rather_than_navigate() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        type_query(&mut app, "j");
        assert_eq!(app.query(), "j", "`j` is a character here, not a move");
    }

    #[test]
    fn enter_keeps_the_filter_but_esc_clears_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        type_query(&mut app, "node");
        press(&mut app, KeyCode::Enter);
        assert!(!app.searching());
        assert_eq!(app.query(), "node");
        assert_eq!(app.visible_count(), 1);

        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Esc);
        assert!(!app.searching());
        assert_eq!(app.query(), "");
        assert_eq!(app.visible_count(), 3);
    }

    #[test]
    fn selection_follows_the_process_across_a_filter_change() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        let before = app.selected_entry().unwrap().pid;

        press(&mut app, KeyCode::Char('/'));
        type_query(&mut app, "s");
        assert_eq!(
            app.selected_entry().unwrap().pid,
            before,
            "the highlighted process should not change out from under the user"
        );
    }

    #[test]
    fn selection_stays_in_bounds_when_the_filter_empties() {
        let mut app = app();
        press(&mut app, KeyCode::Char('G'));
        press(&mut app, KeyCode::Char('/'));
        type_query(&mut app, "zzzz");

        assert_eq!(app.visible_count(), 0);
        assert_eq!(app.selected(), None);
        assert!(app.selected_entry().is_none());
        // Navigating an empty list must not panic.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Up);
    }

    #[test]
    fn q_and_esc_quit_from_normal_mode() {
        for key in [KeyCode::Char('q'), KeyCode::Esc] {
            let mut app = app();
            press(&mut app, key);
            assert!(app.quit, "{key:?} should quit");
        }
    }

    #[test]
    fn ctrl_c_quits_even_while_searching() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit);
    }

    #[test]
    fn key_release_events_are_ignored() {
        let mut app = app();
        let mut release = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        app.handle_key(release);
        assert_eq!(
            app.selected(),
            Some(0),
            "a release must not move the cursor"
        );
    }

    #[test]
    fn killing_a_forbidden_pid_reports_an_error_instead_of_panicking() {
        // PID 1 is refused by the kill guardrails.
        let mut app = App::new(vec![entry(80, 1, "init")]);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.status(), Some(Status::Error(_))));
    }

    #[test]
    fn a_keystroke_clears_the_previous_status() {
        let mut app = App::new(vec![entry(80, 1, "init")]);
        press(&mut app, KeyCode::Enter);
        assert!(app.status().is_some());
        press(&mut app, KeyCode::Char('j'));
        assert!(app.status().is_none());
    }
}
