//! Application state, input handling and background-task orchestration.
//! Network/CLI work always runs on worker threads; results come back as `Event`s.

use crate::config::Config;
use crate::history::{self, Record};
use crate::identity;
use crate::managers::{self, Manager, ManagerStatus};
use crate::model::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc::Sender, Arc};
use std::thread;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Search,
    Installed,
    Updates,
    Orphans,
    History,
    Managers,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Search,
        View::Installed,
        View::Updates,
        View::Orphans,
        View::History,
        View::Managers,
    ];
    pub fn title(self) -> &'static str {
        match self {
            View::Search => "Search",
            View::Installed => "Installed",
            View::Updates => "Updates",
            View::Orphans => "Orphans",
            View::History => "History",
            View::Managers => "Managers",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input,
    Confirm,
    Help,
}

#[derive(Clone)]
pub struct PlannedOp {
    pub op: Op,
    pub kind: ManagerKind,
    pub package: Option<String>,
    pub before: Option<String>,
    pub spec: CommandSpec,
}

pub struct Pending {
    pub title: String,
    pub lines: Vec<String>,
    pub plan: Vec<PlannedOp>,
}

pub enum Event {
    Search { generation: u64, kind: ManagerKind, res: Result<Vec<PackageEntry>, String> },
    Installed(ManagerKind, Result<Vec<PackageEntry>, String>),
    Updates(ManagerKind, Result<Vec<UpdateInfo>, String>),
    Orphans(Result<Vec<String>, String>),
    Info(ManagerKind, String, Result<Vec<(String, String)>, String>),
}

pub struct App {
    pub cfg: Config,
    pub managers: Vec<Arc<dyn Manager>>,
    pub statuses: Vec<ManagerStatus>,
    tx: Sender<Event>,

    pub view: View,
    pub mode: Mode,
    pub input: String,
    pub query: String,
    pub filter: Option<ManagerKind>,

    search_gen: u64,
    search_pending: usize,
    search_errors: Vec<String>,
    results: Vec<PackageEntry>,

    pub installed: HashMap<ManagerKind, Vec<PackageEntry>>,
    pub updates: HashMap<(ManagerKind, String), UpdateInfo>,
    updates_checked: bool,
    updates_pending: usize,
    update_notes: Vec<String>,
    orphans: HashSet<String>,
    orphans_loaded: bool,
    orphans_pending: bool,
    pub info_cache: HashMap<(ManagerKind, String), Vec<(String, String)>>,

    pub rows: Vec<SoftwareIdentity>,
    pub sel: usize,
    pub src: usize,
    pub list_state: ListState,
    pub history: Vec<Record>,

    pub message: String,
    pub confirm: Option<Pending>,
    pub pending_exec: Option<Vec<PlannedOp>>,
    pub should_quit: bool,
    pub dirty: bool,
}

impl App {
    pub fn new(tx: Sender<Event>) -> Self {
        let (cfg, cfg_err) = Config::load();
        let managers = managers::build(&cfg);
        let statuses: Vec<ManagerStatus> =
            managers.iter().map(|m| managers::detect(m.as_ref(), &cfg)).collect();
        let mut app = App {
            cfg,
            managers,
            statuses,
            tx,
            view: View::Search,
            mode: Mode::Normal,
            input: String::new(),
            query: String::new(),
            filter: None,
            search_gen: 0,
            search_pending: 0,
            search_errors: vec![],
            results: vec![],
            installed: HashMap::new(),
            updates: HashMap::new(),
            updates_checked: false,
            updates_pending: 0,
            update_notes: vec![],
            orphans: HashSet::new(),
            orphans_loaded: false,
            orphans_pending: false,
            info_cache: HashMap::new(),
            rows: vec![],
            sel: 0,
            src: 0,
            list_state: ListState::default(),
            history: vec![],
            message: String::new(),
            confirm: None,
            pending_exec: None,
            should_quit: false,
            dirty: true,
        };
        let ready = app.statuses.iter().filter(|s| s.eligible()).count();
        let warnings = app.cfg.warnings();
        app.message = match cfg_err {
            Some(e) => format!("config error (using defaults): {e}"),
            None if !warnings.is_empty() => {
                format!("{ready} manager(s) ready. config warning: {}", warnings.join("; "))
            }
            None => format!("{ready} package manager(s) ready. Press / to search, ? for help."),
        };
        app.reload_installed(None);
        app
    }

    // ---------- queries ----------

    fn manager(&self, kind: ManagerKind) -> &Arc<dyn Manager> {
        self.managers.iter().find(|m| m.kind() == kind).expect("manager registered")
    }

    pub fn eligible(&self, kind: ManagerKind) -> bool {
        self.statuses.iter().any(|s| s.kind == kind && s.eligible())
    }

    fn visible(&self, kind: ManagerKind) -> bool {
        self.eligible(kind) && self.filter.map_or(true, |f| f == kind)
    }

    pub fn busy(&self) -> bool {
        self.search_pending > 0 || self.updates_pending > 0 || self.orphans_pending
    }

    pub fn update_for(&self, e: &PackageEntry) -> Option<&UpdateInfo> {
        self.updates.get(&(e.manager, e.id.clone()))
    }

    pub fn view_len(&self) -> usize {
        match self.view {
            View::History => self.history.len(),
            View::Managers => self.statuses.len(),
            _ => self.rows.len(),
        }
    }

    pub fn src_idx(&self) -> usize {
        match self.rows.get(self.sel) {
            Some(r) => self.src.min(r.entries.len().saturating_sub(1)),
            None => 0,
        }
    }

    pub fn selected(&self) -> Option<(&SoftwareIdentity, &PackageEntry)> {
        let id = self.rows.get(self.sel)?;
        let e = id.entries.get(self.src_idx())?;
        Some((id, e))
    }

    pub fn checked_updates(&self) -> bool {
        self.updates_checked
    }
    pub fn checked_orphans(&self) -> bool {
        self.orphans_loaded
    }

    fn installed_entries(&self) -> Vec<PackageEntry> {
        let mut v: Vec<PackageEntry> = self
            .installed
            .values()
            .flatten()
            .filter(|e| self.visible(e.manager))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.manager.cmp(&b.manager)));
        v
    }

    pub fn rebuild(&mut self) {
        let rows = match self.view {
            View::Search => {
                let lookup: HashMap<(ManagerKind, String), String> = self
                    .installed
                    .values()
                    .flatten()
                    .map(|e| ((e.manager, e.id.clone()), e.installed_version.clone().unwrap_or_default()))
                    .collect();
                let entries: Vec<PackageEntry> = self
                    .results
                    .iter()
                    .filter(|e| self.visible(e.manager))
                    .cloned()
                    .map(|mut e| {
                        if let Some(v) = lookup.get(&(e.manager, e.id.clone())) {
                            e.installed_version = Some(v.clone());
                        }
                        e
                    })
                    .collect();
                identity::group(entries, &self.cfg, &self.query)
            }
            View::Installed => identity::singles(self.installed_entries()),
            View::Updates => identity::singles(
                self.installed_entries()
                    .into_iter()
                    .filter(|e| self.updates.contains_key(&(e.manager, e.id.clone())))
                    .collect(),
            ),
            View::Orphans => identity::singles(
                self.installed_entries()
                    .into_iter()
                    .filter(|e| e.manager == ManagerKind::Pacman && self.orphans.contains(&e.id))
                    .collect(),
            ),
            View::History | View::Managers => vec![],
        };
        self.rows = rows;
        self.clamp();
    }

    fn clamp(&mut self) {
        let len = self.view_len();
        if len == 0 {
            self.sel = 0;
        } else if self.sel >= len {
            self.sel = len - 1;
        }
    }

    // ---------- background tasks ----------

    fn reload_installed(&mut self, only: Option<&[ManagerKind]>) {
        let targets: Vec<Arc<dyn Manager>> = self
            .managers
            .iter()
            .filter(|m| self.eligible(m.kind()) && only.map_or(true, |o| o.contains(&m.kind())))
            .cloned()
            .collect();
        for m in targets {
            let tx = self.tx.clone();
            thread::spawn(move || {
                let kind = m.kind();
                let r = m.installed().map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Installed(kind, r));
            });
        }
    }

    fn submit_search(&mut self) {
        self.search_gen += 1;
        self.results.clear();
        self.search_errors.clear();
        let generation = self.search_gen;
        let q = self.query.clone();
        let targets: Vec<Arc<dyn Manager>> =
            self.managers.iter().filter(|m| self.eligible(m.kind())).cloned().collect();
        self.search_pending = targets.len();
        for m in targets {
            let tx = self.tx.clone();
            let q = q.clone();
            thread::spawn(move || {
                let kind = m.kind();
                let res = m.search(&q).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Search { generation, kind, res });
            });
        }
        self.message = format!("searching “{}” …", self.query);
        self.rebuild();
    }

    fn spawn_updates(&mut self) {
        self.updates_checked = true;
        self.updates.clear();
        self.update_notes.clear();
        let mut targets = Vec::new();
        for m in &self.managers {
            if !self.eligible(m.kind()) {
                continue;
            }
            if m.supports_updates() {
                targets.push(m.clone());
            } else {
                self.update_notes.push(format!("{}: update check not supported", m.kind().label()));
            }
        }
        self.updates_pending = targets.len();
        for m in targets {
            let tx = self.tx.clone();
            thread::spawn(move || {
                let kind = m.kind();
                let r = m.updates().map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Updates(kind, r));
            });
        }
        self.message = "checking for updates …".into();
    }

    fn spawn_orphans(&mut self) {
        self.orphans_loaded = true;
        if !self.eligible(ManagerKind::Pacman) {
            return;
        }
        self.orphans_pending = true;
        let m = self.manager(ManagerKind::Pacman).clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let r = m.orphans().map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Orphans(r));
        });
    }

    fn fetch_info(&mut self) {
        let Some((_, e)) = self.selected() else { return };
        let e = e.clone();
        let key = (e.manager, e.id.clone());
        if self.info_cache.contains_key(&key) {
            return;
        }
        self.message = format!("loading details for {} …", e.id);
        let m = self.manager(e.manager).clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let r = m.info(&e).map_err(|err| format!("{err:#}"));
            let _ = tx.send(Event::Info(e.manager, e.id.clone(), r));
        });
    }

    pub fn on_event(&mut self, ev: Event) {
        self.dirty = true;
        match ev {
            Event::Search { generation, kind, res } => {
                if generation != self.search_gen {
                    return; // stale search
                }
                self.search_pending = self.search_pending.saturating_sub(1);
                match res {
                    Ok(v) => self.results.extend(v.into_iter().take(150)),
                    Err(e) => self.search_errors.push(format!("{}: {e}", kind.label())),
                }
                if self.search_pending == 0 {
                    let n = self.results.len();
                    self.message = if self.search_errors.is_empty() {
                        format!("{n} package entries found")
                    } else {
                        format!("{n} entries; errors: {}", self.search_errors.join("; "))
                    };
                }
                self.rebuild();
            }
            Event::Installed(kind, res) => {
                match res {
                    Ok(v) => {
                        self.installed.insert(kind, v);
                    }
                    Err(e) => self.message = format!("{}: could not list installed packages: {e}", kind.label()),
                }
                self.rebuild();
            }
            Event::Updates(kind, res) => {
                self.updates_pending = self.updates_pending.saturating_sub(1);
                match res {
                    Ok(v) => {
                        for u in v {
                            self.updates.insert((kind, u.id.clone()), u);
                        }
                    }
                    Err(e) => self.update_notes.push(format!("{}: {e}", kind.label())),
                }
                if self.updates_pending == 0 {
                    self.message = format!("{} update(s) available", self.updates.len());
                    if !self.update_notes.is_empty() {
                        self.message.push_str(&format!(" ({})", self.update_notes.join("; ")));
                    }
                }
                self.rebuild();
            }
            Event::Orphans(res) => {
                self.orphans_pending = false;
                match res {
                    Ok(v) => {
                        self.message = format!("{} potentially unused package(s) — review before removing", v.len());
                        self.orphans = v.into_iter().collect();
                    }
                    Err(e) => self.message = format!("orphans: {e}"),
                }
                self.rebuild();
            }
            Event::Info(kind, id, res) => match res {
                Ok(v) => {
                    self.info_cache.insert((kind, id), v);
                    self.message.clear();
                }
                Err(e) => self.message = format!("details: {e}"),
            },
        }
    }

    // ---------- views ----------

    fn set_view(&mut self, v: View) {
        self.view = v;
        self.sel = 0;
        self.src = 0;
        match v {
            View::Updates if !self.updates_checked => self.spawn_updates(),
            View::Orphans if !self.orphans_loaded => self.spawn_orphans(),
            View::History => self.history = history::load(500),
            _ => {}
        }
        self.rebuild();
    }

    fn move_sel(&mut self, d: isize) {
        let len = self.view_len();
        if len == 0 {
            return;
        }
        self.sel = (self.sel as isize + d).clamp(0, len as isize - 1) as usize;
        self.src = 0;
    }

    fn cycle_src(&mut self, d: isize) {
        if let Some(r) = self.rows.get(self.sel) {
            let n = r.entries.len() as isize;
            if n > 1 {
                self.src = (self.src_idx() as isize + d).rem_euclid(n) as usize;
            }
        }
    }

    fn cycle_filter(&mut self) {
        self.filter = match self.filter {
            None => Some(ManagerKind::ALL[0]),
            Some(k) => ManagerKind::ALL
                .iter()
                .position(|x| *x == k)
                .and_then(|i| ManagerKind::ALL.get(i + 1))
                .copied(),
        };
        self.rebuild();
        self.message = format!("filter: {}", self.filter.map_or("all", |k| k.label()));
    }

    fn refresh(&mut self) {
        match self.view {
            View::Search if !self.query.is_empty() => self.submit_search(),
            View::Installed => self.reload_installed(None),
            View::Updates => self.spawn_updates(),
            View::Orphans => self.spawn_orphans(),
            View::History => self.history = history::load(500),
            _ => {}
        }
    }

    fn toggle_manager(&mut self) {
        if self.view != View::Managers {
            return;
        }
        let Some(s) = self.statuses.get_mut(self.sel) else { return };
        s.enabled = !s.enabled;
        let (kind, on) = (s.kind, s.enabled);
        self.message = format!("{} {} (this session only; edit config.toml to persist)", kind.label(), if on { "enabled" } else { "disabled" });
        if on {
            self.reload_installed(Some(&[kind]));
        }
        self.rebuild();
    }

    // ---------- actions ----------

    fn request(&mut self, op: Op) {
        if matches!(self.view, View::History | View::Managers) {
            return;
        }
        if op == Op::UpgradeAll {
            return self.request_upgrade_all();
        }
        let sel = self.selected().map(|(i, e)| (i.name.clone(), e.clone()));
        let Some((name, entry)) = sel else {
            self.message = "nothing selected".into();
            return;
        };
        match op {
            Op::Install if entry.is_installed() => {
                self.message = format!("{} is already installed via {}", name, entry.manager.label());
                return;
            }
            Op::Remove | Op::Upgrade if !entry.is_installed() => {
                self.message = format!(
                    "the {} source of {} is not installed (Tab switches source)",
                    entry.manager.label(),
                    name
                );
                return;
            }
            _ => {}
        }
        let spec = match self.manager(entry.manager).plan(op, Some(&entry)) {
            Ok(s) => s,
            Err(e) => {
                self.message = format!("{e:#}");
                return;
            }
        };
        let verb = match op {
            Op::Install => "Install",
            Op::Remove => "Remove",
            _ => "Upgrade",
        };
        let mut lines = vec![
            format!("{verb} “{name}” via {}", entry.manager.label()),
            format!("Package id: {}", entry.id),
            format!("Command:    {}", spec.display()),
            if spec.root {
                "Privileges: root required — sudo will prompt".to_string()
            } else {
                "Privileges: runs as your user (the manager handles its own elevation)".to_string()
            },
        ];
        if op == Op::Upgrade {
            if let Some(u) = self.update_for(&entry) {
                lines.push(format!("Update:     {} → {}", u.from, u.to));
            }
        }
        if op == Op::Remove {
            lines.push(format!("Only the {} installation is removed; other sources are untouched.", entry.manager.label()));
        }
        if let Some(n) = &spec.note {
            lines.push(format!("Note: {n}"));
        }
        self.confirm = Some(Pending {
            title: format!(" Confirm {} ", op),
            lines,
            plan: vec![PlannedOp {
                op,
                kind: entry.manager,
                package: Some(entry.id.clone()),
                before: entry.installed_version.clone(),
                spec,
            }],
        });
        self.mode = Mode::Confirm;
    }

    fn request_upgrade_all(&mut self) {
        let mut plan: Vec<PlannedOp> = Vec::new();
        let mut lines = vec![
            "Upgrade everything. Managers run one after another;".to_string(),
            "a failure in one does not stop the others.".to_string(),
            String::new(),
        ];
        for m in &self.managers {
            let k = m.kind();
            if !self.eligible(k) {
                continue;
            }
            match m.plan(Op::UpgradeAll, None) {
                Ok(spec) => {
                    lines.push(format!("[{}] {}", k.label(), spec.display()));
                    plan.push(PlannedOp { op: Op::UpgradeAll, kind: k, package: None, before: None, spec });
                }
                Err(e) => lines.push(format!("[{}] skipped: {e:#}", k.label())),
            }
        }
        if plan.is_empty() {
            self.message = "no manager can upgrade".into();
            return;
        }
        plan.sort_by_key(|p| self.cfg.rank(p.kind));
        self.confirm = Some(Pending { title: " Confirm upgrade all ".into(), lines, plan });
        self.mode = Mode::Confirm;
    }

    /// Called by the main loop after the terminal ran the plan.
    pub fn after_exec(&mut self, plan: &[PlannedOp], outcomes: &[OpStatus]) {
        let mut kinds: Vec<ManagerKind> = plan.iter().map(|p| p.kind).collect();
        if kinds.iter().any(|k| matches!(k, ManagerKind::Pacman | ManagerKind::Aur)) {
            kinds.push(ManagerKind::Pacman);
            kinds.push(ManagerKind::Aur);
        }
        kinds.sort();
        kinds.dedup();
        self.updates.retain(|(k, _), _| !kinds.contains(k));
        self.updates_checked = false;
        self.orphans_loaded = false;
        self.info_cache.clear();
        self.reload_installed(Some(&kinds));
        let ok = outcomes.iter().filter(|s| **s == OpStatus::Successful).count();
        let view = self.view;
        self.set_view(view);
        self.message = if ok == outcomes.len() {
            format!("all {ok} operation(s) succeeded")
        } else {
            format!("{ok} of {} operation(s) succeeded — see History", outcomes.len())
        };
    }

    // ---------- keys ----------

    pub fn on_key(&mut self, k: KeyEvent) {
        self.dirty = true;
        match self.mode {
            Mode::Input => self.key_input(k),
            Mode::Confirm => self.key_confirm(k),
            Mode::Help => self.mode = Mode::Normal,
            Mode::Normal => self.key_normal(k),
        }
    }

    fn key_input(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.query = self.input.trim().to_string();
                if !self.query.is_empty() {
                    self.set_view(View::Search);
                    self.submit_search();
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char('u') if ctrl => self.input.clear(),
            KeyCode::Char(c) if !ctrl => self.input.push(c),
            _ => {}
        }
    }

    fn key_confirm(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(p) = self.confirm.take() {
                    self.pending_exec = Some(p.plan);
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                self.confirm = None;
                self.mode = Mode::Normal;
                self.message = "cancelled".into();
            }
            _ => {}
        }
    }

    fn key_normal(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Char('d') if ctrl => self.move_sel(10),
            KeyCode::Char('u') if ctrl => self.move_sel(-10),
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::Char('g') | KeyCode::Home => self.move_sel(isize::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => self.move_sel(isize::MAX / 2),
            KeyCode::Tab => self.cycle_src(1),
            KeyCode::BackTab => self.cycle_src(-1),
            KeyCode::Char('/') => {
                self.input = self.query.clone();
                self.mode = Mode::Input;
            }
            KeyCode::Char(c @ '1'..='6') => self.set_view(View::ALL[(c as u8 - b'1') as usize]),
            KeyCode::Char('f') => self.cycle_filter(),
            KeyCode::Char('i') => self.request(Op::Install),
            KeyCode::Char('r') => self.request(Op::Remove),
            KeyCode::Char('U') => self.request(Op::Upgrade),
            KeyCode::Char('A') => self.request(Op::UpgradeAll),
            KeyCode::Char('d') | KeyCode::Enter => self.fetch_info(),
            KeyCode::Char('R') => self.refresh(),
            KeyCode::Char('e') => self.toggle_manager(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
    }
}

/// Runs a confirmed plan with the real terminal attached (caller has already
/// left the alternate screen). Every operation is attempted; failures never
/// abort the remaining ones (spec 8.4 / 18). Each result goes to the history.
pub fn execute_plan(plan: &[PlannedOp]) -> Vec<OpStatus> {
    use crate::exec;
    use std::os::unix::process::ExitStatusExt;

    let mut results = Vec::new();
    let mut history_errors: Vec<String> = Vec::new();
    for p in plan {
        println!("\n\x1b[1m==> [{}] {}\x1b[0m\n    $ {}\n", p.kind.label(), p.op, p.spec.display());
        let started = std::time::Instant::now();
        // Success is judged by the real exit status, never by the process having
        // merely been launched. Launch failures are told apart from manager failures.
        let (status, detail, exit_code) = match exec::run_interactive(&p.spec) {
            Ok(s) if s.success() => (OpStatus::Successful, None, s.code()),
            Ok(s) if s.signal() == Some(libc::SIGINT) || s.code() == Some(130) => {
                (OpStatus::Cancelled, Some("interrupted".to_string()), s.code())
            }
            Ok(s) => (
                OpStatus::Failed,
                Some(format!(
                    "`{}` exited with status {}",
                    p.spec.program,
                    s.code().map_or("signal".to_string(), |c| c.to_string())
                )),
                s.code(),
            ),
            Err(e) => (
                OpStatus::Failed,
                Some(format!("could not launch `{}`: {e}", p.spec.program)),
                None,
            ),
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        if let Err(e) = history::append(&Record {
            v: history::SCHEMA,
            ts: history::now(),
            op: p.op.to_string(),
            manager: p.kind.label().to_string(),
            package: p.package.clone(),
            before: p.before.clone(),
            after: None,
            status: status.to_string(),
            detail,
            exit_code,
            duration_ms: Some(duration_ms),
        }) {
            history_errors.push(format!("[{}] {}: {e}", p.kind.label(), p.op));
        }
        results.push(status);
    }

    println!("\n\x1b[1m==> Summary\x1b[0m");
    for (p, s) in plan.iter().zip(&results) {
        println!("    [{:<8}] {:<12} {}", p.kind.label(), p.op.to_string(), s);
    }
    let ok = results.iter().filter(|s| **s == OpStatus::Successful).count();
    let overall = if ok == results.len() {
        OpStatus::Successful
    } else if ok == 0 {
        if results.iter().all(|s| *s == OpStatus::Cancelled) {
            OpStatus::Cancelled
        } else {
            OpStatus::Failed
        }
    } else {
        OpStatus::PartiallySuccessful
    };
    println!("    Overall: {overall}");
    for e in &history_errors {
        println!("    warning: could not write history for {e}");
    }
    println!("\nPress Enter to return to Zito …");
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    results
}
