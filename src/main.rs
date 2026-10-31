mod app;
mod config;
mod exec;
mod history;
mod identity;
mod managers;
mod model;
mod ui;

use anyhow::Result;
use app::{execute_plan, App, Event};
use ratatui::crossterm::event::{self, Event as CEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

fn main() -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(tx);
    let mut terminal = ratatui::init(); // also installs a panic hook that restores the tty
    let res = run(&mut terminal, &mut app, &rx);
    ratatui::restore();
    res
}

fn run(terminal: &mut DefaultTerminal, app: &mut App, rx: &Receiver<Event>) -> Result<()> {
    loop {
        while let Ok(ev) = rx.try_recv() {
            app.on_event(ev);
        }
        // Redraw only when something changed: no busy loop, no daemon.
        if app.dirty {
            terminal.draw(|f| ui::draw(f, app))?;
            app.dirty = false;
        }
        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                CEvent::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                CEvent::Resize(..) => app.dirty = true,
                _ => {}
            }
        }
        if let Some(plan) = app.pending_exec.take() {
            ratatui::restore();
            let outcomes = execute_plan(&plan);
            *terminal = ratatui::init();
            terminal.clear()?;
            app.after_exec(&plan, &outcomes);
        }
        if app.should_quit {
            return Ok(());
        }
    }
}
