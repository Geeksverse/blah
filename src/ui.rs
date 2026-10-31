use crate::app::{App, Mode, View};
use crate::history::{self, Record};
use crate::managers::ManagerStatus;
use crate::model::*;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

pub fn label_color(k: ManagerKind) -> Color {
    match k {
        ManagerKind::Pacman => Color::Blue,
        ManagerKind::Aur => Color::Magenta,
        ManagerKind::Flatpak => Color::Green,
        ManagerKind::Npm => Color::Red,
        ManagerKind::Cargo => Color::Yellow,
    }
}

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    } else {
        s.to_string()
    }
}

fn label_span(k: ManagerKind) -> Span<'static> {
    Span::styled(format!("[{}]", k.label()), Style::new().fg(label_color(k)).add_modifier(Modifier::BOLD))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(5), Constraint::Length(1)]).split(f.area());
    draw_header(f, app, rows[0]);
    let body = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(rows[1]);
    draw_list(f, app, body[0]);
    draw_sidebar(f, app, body[1]);
    draw_status(f, app, rows[2]);
    match app.mode {
        Mode::Confirm => draw_confirm(f, app),
        Mode::Help => draw_help(f),
        _ => {}
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Min(20), Constraint::Length(38)]).split(area);
    let mut spans = Vec::new();
    for (i, v) in View::ALL.iter().enumerate() {
        let st = if *v == app.view {
            Style::new().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::Gray)
        };
        spans.push(Span::styled(format!(" {} {} ", i + 1, v.title()), st));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::bordered().title(" Zito ").border_style(Style::new().fg(Color::Cyan))),
        cols[0],
    );

    let filter = app.filter.map_or("all", |k| k.label());
    let content = if app.mode == Mode::Input {
        Line::from(vec![Span::raw(app.input.clone()), Span::styled("█", Style::new().fg(Color::Cyan))])
    } else if app.query.is_empty() {
        Line::from(Span::styled("press / to search", dim()))
    } else {
        Line::from(app.query.clone())
    };
    let border = if app.mode == Mode::Input { Color::Yellow } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(content)
            .block(Block::bordered().title(format!(" / search · filter: {filter} ")).border_style(Style::new().fg(border))),
        cols[1],
    );
}

fn identity_line(app: &App, id: &SoftwareIdentity) -> Line<'static> {
    let d = &id.entries[0];
    let mut spans = vec![
        Span::styled(format!("{:<26}", trunc(&id.name, 25)), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{:<9}", format!("[{}]", d.manager.label())),
            Style::new().fg(label_color(d.manager)).add_modifier(Modifier::BOLD),
        ),
    ];
    if id.entries.len() > 1 {
        let alts: Vec<&str> = id.entries[1..].iter().map(|e| e.manager.label()).collect();
        spans.push(Span::styled(format!("+{} ", alts.join(" +")), dim()));
    }
    if let Some(i) = id.entries.iter().find(|e| e.is_installed()) {
        let mut s = format!("installed {}", i.installed_version.clone().unwrap_or_default());
        if i.manager != d.manager {
            s.push_str(&format!(" via {}", i.manager.label()));
        }
        if i.explicit == Some(false) {
            s.push_str(" (dep)");
        }
        spans.push(Span::styled(s, Style::new().fg(Color::Green)));
        if let Some(u) = app.update_for(i) {
            spans.push(Span::styled(format!("  ↑ {}", u.to), Style::new().fg(Color::Yellow)));
        }
    } else if let Some(v) = &d.version {
        spans.push(Span::styled(v.clone(), dim()));
    }
    Line::from(spans)
}

fn history_line(r: &Record) -> Line<'static> {
    let color = match r.status.as_str() {
        "successful" => Color::Green,
        "failed" => Color::Red,
        _ => Color::Yellow,
    };
    Line::from(vec![
        Span::styled(format!("{}  ", history::fmt_ts(r.ts)), dim()),
        Span::styled(format!("{:<11}", r.status), Style::new().fg(color)),
        Span::raw(format!("{:<12}", r.op)),
        Span::raw(format!("{:<9}", r.manager)),
        Span::raw(r.package.clone().unwrap_or_else(|| "(all)".into())),
    ])
}

fn manager_line(s: &ManagerStatus) -> Line<'static> {
    let color = match s.state() {
        "ready" => Color::Green,
        "disabled" => Color::Yellow,
        _ => Color::Red,
    };
    Line::from(vec![
        Span::styled(format!("{:<9}", format!("[{}]", s.kind.label())), Style::new().fg(label_color(s.kind)).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<17}", s.state()), Style::new().fg(color)),
        Span::styled(trunc(s.version.as_deref().unwrap_or(""), 40), dim()),
    ])
}

fn empty_hint(app: &App) -> String {
    match app.view {
        View::Search if app.query.is_empty() => "Press / and type a name to search every ready package manager at once.".into(),
        View::Search => "No results.".into(),
        View::Installed => "No installed packages loaded yet (or none match the filter).".into(),
        View::Updates if app.checked_updates() && !app.busy() => "Everything checked is up to date.".into(),
        View::Updates => "Checking for updates …".into(),
        View::Orphans if app.checked_orphans() && !app.busy() => "No orphaned packages found.".into(),
        View::Orphans => "Looking for orphans …".into(),
        View::History => "No operations recorded yet.".into(),
        View::Managers => "No managers.".into(),
    }
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem<'static>> = match app.view {
        View::History => app.history.iter().map(|r| ListItem::new(history_line(r))).collect(),
        View::Managers => app.statuses.iter().map(|s| ListItem::new(manager_line(s))).collect(),
        _ => app.rows.iter().map(|r| ListItem::new(identity_line(app, r))).collect(),
    };
    let title = format!(" {} ({}){} ", app.view.title(), items.len(), if app.busy() { " · working…" } else { "" });
    let block = Block::bordered().title(title).border_style(Style::new().fg(Color::Cyan));
    if items.is_empty() {
        f.render_widget(Paragraph::new(empty_hint(app)).block(block).wrap(Wrap { trim: true }), area);
        return;
    }
    app.list_state.select(Some(app.sel));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▌ ");
    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn kv_lines(l: &mut Vec<Line<'static>>, kv: &[(String, String)], max: usize) {
    for (k, v) in kv.iter().take(max) {
        l.push(Line::from(vec![
            Span::styled(format!("{k}: "), Style::new().fg(Color::Cyan)),
            Span::raw(trunc(v, 300)),
        ]));
    }
    if kv.len() > max {
        l.push(Line::from(Span::styled(format!("… {} more field(s)", kv.len() - max), dim())));
    }
}

fn header(t: &str) -> Line<'static> {
    Line::from(Span::styled(t.to_string(), Style::new().add_modifier(Modifier::UNDERLINED)))
}

fn details_lines(app: &App) -> Vec<Line<'static>> {
    match app.view {
        View::History => {
            let Some(r) = app.history.get(app.sel) else { return vec![Line::raw("No selection")] };
            let mut l = vec![Line::from(Span::styled(r.package.clone().unwrap_or_else(|| "(all)".into()), Style::new().add_modifier(Modifier::BOLD)))];
            let kv = vec![
                ("When".to_string(), history::fmt_ts(r.ts) + " UTC"),
                ("Operation".to_string(), r.op.clone()),
                ("Manager".to_string(), r.manager.clone()),
                ("Status".to_string(), r.status.clone()),
                ("Before".to_string(), r.before.clone().unwrap_or_default()),
                ("Exit code".to_string(), r.exit_code.map(|c| c.to_string()).unwrap_or_default()),
                ("Duration".to_string(), r.duration_ms.map(|d| format!("{:.1}s", d as f64 / 1000.0)).unwrap_or_default()),
                ("Detail".to_string(), r.detail.clone().unwrap_or_default()),
            ];
            let kv: Vec<_> = kv.into_iter().filter(|(_, v)| !v.is_empty()).collect();
            kv_lines(&mut l, &kv, 20);
            l
        }
        View::Managers => {
            let Some(s) = app.statuses.get(app.sel) else { return vec![Line::raw("No selection")] };
            let mut l = vec![Line::from(vec![label_span(s.kind), Span::raw(format!(" {}", s.state()))]), Line::raw("")];
            let kv = vec![
                ("Binary".to_string(), s.binary.clone()),
                ("Path".to_string(), s.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
                ("Version".to_string(), s.version.clone().unwrap_or_default()),
                ("Detected".to_string(), s.path.is_some().to_string()),
                ("Operational".to_string(), s.operational.to_string()),
                ("Enabled".to_string(), s.enabled.to_string()),
                ("Note".to_string(), s.note.clone().unwrap_or_default()),
            ];
            let kv: Vec<_> = kv.into_iter().filter(|(_, v)| !v.is_empty()).collect();
            kv_lines(&mut l, &kv, 20);
            l
        }
        _ => package_details(app),
    }
}

fn package_details(app: &App) -> Vec<Line<'static>> {
    let Some((id, sel)) = app.selected() else { return vec![Line::raw("No selection")] };
    let cur = app.src_idx();
    let mut l = vec![Line::from(Span::styled(id.name.clone(), Style::new().add_modifier(Modifier::BOLD)))];
    if !id.description.is_empty() {
        l.push(Line::raw(id.description.clone()));
    }
    l.push(Line::raw(""));
    l.push(header("Sources"));
    for (i, e) in id.entries.iter().enumerate() {
        let mut s = vec![
            Span::raw(if i == cur { "▶ " } else { "  " }),
            label_span(e.manager),
            Span::raw(format!(" {}", e.id)),
        ];
        if let Some(v) = &e.installed_version {
            s.push(Span::styled(format!("  installed {v}"), Style::new().fg(Color::Green)));
        } else if let Some(v) = &e.version {
            s.push(Span::styled(format!("  {v}"), dim()));
        }
        if let Some(u) = app.update_for(e) {
            s.push(Span::styled(format!("  ↑ {}", u.to), Style::new().fg(Color::Yellow)));
        }
        if let Some(r) = &e.repo {
            s.push(Span::styled(format!("  ({r})"), dim()));
        }
        l.push(Line::from(s));
    }
    if let Some(why) = id.reason.explain() {
        l.push(Line::from(Span::styled(why, dim())));
    }
    l.push(Line::raw(""));
    match app.info_cache.get(&(sel.manager, sel.id.clone())) {
        Some(kv) => {
            l.push(header("Details"));
            kv_lines(&mut l, kv, 16);
        }
        None => l.push(Line::from(Span::styled("press d for full details from the manager", dim()))),
    }
    l
}

fn key_line(key: &str, text: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<6}"), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(text),
    ])
}

fn options_lines(app: &App) -> Vec<Line<'static>> {
    match app.view {
        View::Managers => return vec![key_line("e", "enable / disable (session)".into()), key_line("R", "re-detect: restart Zito".into())],
        View::History => return vec![key_line("R", "reload history".into())],
        _ => {}
    }
    let Some((id, e)) = app.selected() else { return vec![Line::from(Span::styled("(nothing selected)", dim()))] };
    let m = e.manager.label();
    let mut l = Vec::new();
    if e.is_installed() {
        l.push(key_line("r", format!("Remove via {m}")));
        l.push(key_line("U", format!("Upgrade via {m}")));
    } else {
        l.push(key_line("i", format!("Install via {m}")));
    }
    if id.entries.len() > 1 {
        l.push(key_line("Tab", format!("Switch source ({} available)", id.entries.len())));
    }
    l.push(key_line("d", "Package details".into()));
    l.push(key_line("A", "Upgrade all managers".into()));
    l
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let opts = options_lines(app);
    let oh = (opts.len() as u16 + 2).min(area.height / 2);
    let chunks = Layout::vertical([Constraint::Min(5), Constraint::Length(oh)]).split(area);
    f.render_widget(
        Paragraph::new(details_lines(app)).block(Block::bordered().title(" Details ")).wrap(Wrap { trim: false }),
        chunks[0],
    );
    f.render_widget(Paragraph::new(opts).block(Block::bordered().title(" Options ")), chunks[1]);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let text = if app.message.is_empty() {
        " /:search  1-6:views  Tab:source  i:install  r:remove  U:upgrade  A:all  f:filter  ?:help  q:quit".to_string()
    } else {
        format!(" {}", app.message)
    };
    f.render_widget(Paragraph::new(text).style(Style::new().bg(Color::Black).fg(Color::Gray)), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn draw_confirm(f: &mut Frame, app: &App) {
    let Some(p) = &app.confirm else { return };
    let mut lines: Vec<Line<'static>> = p.lines.iter().map(|s| Line::raw(s.clone())).collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("[y] confirm", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("    "),
        Span::styled("[n] cancel", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
    ]));
    let area = centered(f.area(), 86, lines.len() as u16 + 4);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(p.title.clone()).border_style(Style::new().fg(Color::Yellow)))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_help(f: &mut Frame) {
    let rows = [
        ("j k ↑ ↓", "move selection"),
        ("g G", "top / bottom"),
        ("^d ^u", "half page down / up"),
        ("/", "search all ready managers"),
        ("1-6", "Search, Installed, Updates, Orphans, History, Managers"),
        ("Tab", "cycle package source of the selected software"),
        ("f", "cycle manager filter"),
        ("d Enter", "load full package details"),
        ("i", "install selected source"),
        ("r", "remove selected source"),
        ("U", "upgrade selected source"),
        ("A", "upgrade everything (per-manager, failures isolated)"),
        ("R", "refresh current view"),
        ("e", "enable/disable manager (Managers view)"),
        ("q", "quit"),
    ];
    let mut lines: Vec<Line<'static>> = rows.iter().map(|(k, d)| key_line_w(k, d)).collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("any key closes this help", dim())));
    let area = centered(f.area(), 76, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines).block(Block::bordered().title(" Keys ")), area);
}

fn key_line_w(k: &str, d: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<10}"), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(d.to_string()),
    ])
}
