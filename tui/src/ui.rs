use crate::{
    api::{CurrentPolicy, Detail, Hardware, Wsl, METRICS, RANGES},
    app::{policy_values, preview, Dashboard, Modal, Page, POLICY_FIELDS},
    config::Layout,
    live_ui,
    theme::{data_style, style, Token},
};
use ratatui::{
    layout::{Alignment, Constraint, Layout as Split, Rect},
    style::{Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Cell, Chart, Clear, Dataset, GraphType,
        HighlightSpacing, Paragraph, Row, Table, TableState, Wrap,
    },
    Frame,
};
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_width::UnicodeWidthChar;

const NA: &str = "n/a";
const GIB: u64 = 1 << 30;

pub fn draw(frame: &mut Frame, app: &mut Dashboard) {
    let area = frame.area();
    if app.restarted.is_some() {
        centered(frame, area, "WSL restarting · reconnect in ~90 s");
        return;
    }
    app.too_small = area.width < 80 || area.height < 24;
    if app.too_small {
        centered(frame, area, &format!("cm-top needs 80×24, window is {}×{} · q quit", area.width, area.height));
        return;
    }
    app.narrow = area.width < 100;
    let [header, body, message, footer] = Split::vertical([
        Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1), Constraint::Length(1),
    ]).areas(area);
    draw_header(frame, app, header);
    let body = match (app.hardware_live, app.layout) {
        (true, _) => body,
        (false, Layout::Console) if !app.narrow => {
            let [nav, body] = Split::horizontal([Constraint::Length(16), Constraint::Fill(1)]).areas(body);
            sidebar(frame, app, nav);
            body
        }
        (false, Layout::Console | Layout::Budget) => {
            let [nav, body] = Split::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(body);
            tabs(frame, app, nav);
            body
        }
        (false, Layout::Dashboard) => body,
    };
    if app.hardware_live {
        live_ui::fullscreen(frame, app, body);
    } else if !app.loaded || app.data.system.is_none() && app.data.identity.is_none() {
        placeholder(frame, app, body, app.tab.label());
    } else {
        render_page(app.tab, frame, body, app);
    }
    if app.stale.is_some() || app.modal.is_some() {
        frame.buffer_mut().set_style(body, Style::default().add_modifier(Modifier::DIM));
    }
    draw_message(frame, app, message);
    let [hints, global] = Split::horizontal([Constraint::Fill(1), Constraint::Length(17)]).areas(footer);
    frame.render_widget(Paragraph::new(app.shortcuts()), hints);
    frame.render_widget(Paragraph::new("? help  q quit ").right_aligned(), global);
    if let Some(modal) = &app.modal {
        draw_modal(frame, app, modal);
    }
}

fn centered(frame: &mut Frame, area: Rect, message: &str) {
    let height = area.height.min(3);
    let top = area.y + (area.height - height) / 2;
    frame.render_widget(Paragraph::new(message).centered().wrap(Wrap { trim: true })
        .style(style(Token::Title)), Rect::new(area.x, top, area.width, height));
}

pub(crate) fn block(title: impl Into<Line<'static>>, focus: bool) -> Block<'static> {
    Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
        .title(title.into().style(style(Token::Title)))
        .border_style(style(if focus { Token::Focus } else { Token::Faint }))
}

fn draw_header(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let system = app.data.system.as_ref();
    let host = system.map_or(NA.into(), |s| clean(&s.hostname));
    let role = app.data.identity.as_ref().map_or(NA.into(), |me| clean(&me.role));
    let oomd = system.and_then(|s| s.oomd.as_ref()?.active);
    let stale = app.stale.map(|since| format!("STALE {}s  ", since.elapsed().as_secs())).unwrap_or_default();
    let layout = if !app.narrow { format!("F{} {}  ", app.layout as usize + 1, app.layout.name()) } else { String::new() };
    let right = Line::from(vec![
        Span::styled(stale, style(Token::Warn).add_modifier(Modifier::BOLD)),
        Span::raw(layout), Span::raw("oomd "),
        status(oomd, false, "✓", "✗", Token::Success, Token::Error),
        Span::raw(format!("  role {role}  ")),
    ]);
    let [left, right_area] = Split::horizontal([Constraint::Fill(1), Constraint::Length(right.width() as u16)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(vec![
        Span::styled("  cm-top ", style(Token::Accent).add_modifier(Modifier::BOLD)),
        Span::raw(format!("{host}  up {}", relative(system.and_then(|s| s.uptime_s)))),
    ])), left);
    frame.render_widget(Paragraph::new(right), right_area);
}

fn draw_message(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let (message, token) = if !app.data.errors.is_empty() || app.detail_error.is_some() {
        let error = app.data.errors.first().map(String::as_str).or(app.detail_error.as_deref()).unwrap_or_default();
        let socket = error.contains("/run/compute-mgr/api.sock") || app.data.system.is_none();
        let text = if socket {
            "✗ daemon unreachable (/run/compute-mgr/api.sock) · r retry".into()
        } else {
            format!("✗ {} · r retry", clean(error).chars().take(usize::from(area.width.saturating_sub(13))).collect::<String>())
        };
        (text, Token::Error)
    } else if app.busy {
        let step = app.busy_since.map_or(0, |time| (time.elapsed().as_millis() / 100) as usize);
        (format!("{} Applying change", ['|', '/', '-', '\\'][step % 4]), Token::Info)
    } else if app.filtering || !app.filter.is_empty() {
        (format!("/{}{} · esc clear", clean(&app.filter), if app.filtering { "_" } else { "" }), Token::Info)
    } else if let Some(result) = &app.status {
        match result {
            Ok(text) => (format!("✓ {}", clean(text)), Token::Success),
            Err(text) => (format!("✗ {} · r retry", clean(text)), Token::Error),
        }
    } else if !app.loaded || app.view() != crate::api::View::None && app.detail.is_none() {
        ("Loading data".into(), Token::Faint)
    } else {
        match app.tab {
            Page::Hardware if !app.hardware_live => ("Windows values cached 1h".into(), Token::Faint),
            Page::Overview if app.layout == Layout::Budget && app.is_admin() => ("Policy values apply live".into(), Token::Faint),
            _ => (String::new(), Token::Faint),
        }
    };
    frame.render_widget(Paragraph::new(format!(" {message}")).style(style(token)), area);
}

fn sidebar(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let lines: Vec<_> = Page::ALL.into_iter().filter(|page| page.visible(app.is_admin())).map(|page| {
        let selected = page == app.tab;
        let focused = app.pane == 1;
        let prefix = if selected && focused { "❯ " } else { "  " };
        Line::styled(format!("{prefix}{:<12}", page.label()), selection(selected, focused))
    }).collect();
    frame.render_widget(Paragraph::new(lines).block(block(" Pages ", app.pane == 1)), area);
}

fn tabs(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let tabs = Page::ALL.into_iter().filter(|page| page.visible(app.is_admin())).map(|page| {
        let label = match (app.narrow, page) {
            (true, Page::Hardware) => "5 HW",
            (true, Page::History) => "8 Hist",
            _ => page.label(),
        };
        Span::styled(format!(" {label} "), selection(page == app.tab, true))
    }).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(tabs)), area);
}

fn selection(selected: bool, focused: bool) -> Style {
    match (selected, focused) {
        (true, true) => style(Token::Selection),
        (true, false) => style(Token::Selection).add_modifier(Modifier::DIM),
        _ => Style::default(),
    }
}

fn render_page(page: Page, frame: &mut Frame, area: Rect, app: &mut Dashboard) {
    let focused = app.layout != Layout::Console || app.pane == 0;
    match page {
        Page::Overview => {
            let lower = live_ui::strip(frame, app, area);
            overview(frame, app, lower, app.layout == Layout::Budget, focused);
        }
        Page::Users => users(frame, app, area, false, focused),
        Page::Kernels => kernels(frame, app, area, false, focused),
        Page::Policy => policy(frame, app, area, focused),
        Page::Hardware => hardware(frame, app, area, focused),
        Page::Wsl => wsl(frame, app, area, focused),
        Page::Envs => envs(frame, app, area, focused),
        Page::History => history(frame, app, area),
        Page::Audit => audit(frame, app, area, focused),
    }
}

fn glyph(ch: char) -> char {
    if ch.width() == Some(1) && !ch.is_control() && ch != '\u{1b}' { ch } else { '?' }
}

pub(crate) fn clean(text: &str) -> String { text.chars().map(glyph).collect() }

pub(crate) fn size(value: Option<u64>) -> String {
    let Some(value) = value else { return NA.into(); };
    let n = value as f64;
    match value {
        0 => "0.0G".into(),
        1..=1023 => format!("{n:.0}B"),
        1024..=1048575 => format!("{:.1}K", n / 1024.0),
        1048576..=1073741823 => format!("{:.1}M", n / 1048576.0),
        _ if value < (1 << 40) => format!("{:.1}G", n / GIB as f64),
        _ => format!("{:.1}T", n / (1u64 << 40) as f64),
    }
}

fn bytes(value: Option<u64>) -> String { size(value) }

fn relative(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds.filter(|n| n.is_finite() && *n >= 0.0) else { return NA.into(); };
    let seconds = seconds as u64;
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

fn age(app: &Dashboard, time: Option<f64>) -> String {
    let now = app.data.system.as_ref().and_then(|s| s.time).unwrap_or_else(||
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64());
    relative(time.map(|t| (now - t).max(0.0)))
}

pub(crate) fn percent(value: Option<f64>) -> String {
    value.filter(|value| value.is_finite()).map_or(NA.into(), |n| format!("{n:.0}%"))
}

fn count(value: Option<u64>) -> String { value.map_or(NA.into(), |n| n.to_string()) }

pub(crate) fn ratio(used: Option<u64>, total: Option<u64>) -> Option<f64> {
    let (used, total) = (used?, total?);
    if total == 0 { return (used == 0).then_some(0.0); }
    Some(100.0 * used as f64 / total as f64)
}

fn status(value: Option<bool>, selected: bool, yes: &'static str, no: &'static str, on: Token, off: Token) -> Span<'static> {
    let (text, token) = match value {
        Some(true) => (yes, on),
        Some(false) => (no, off),
        None => (NA, Token::Faint),
    };
    Span::styled(text, data_style(token, selected))
}

fn blank(label: &str) -> Line<'static> { Line::styled(label.to_owned(), style(Token::Faint)) }

fn panel(frame: &mut Frame, area: Rect, title: impl Into<Line<'static>>, lines: Vec<Line<'static>>, focused: bool) {
    frame.render_widget(Paragraph::new(lines).block(block(title, focused)), area);
}

fn placeholder(frame: &mut Frame, app: &Dashboard, area: Rect, title: &str) {
    let lines = if app.stale.is_some() {
        vec![blank(" Data unavailable · r retry")]
    } else {
        [" ████████  ████", " █████     █████", " ███████   ████", " █████     ███"].into_iter().map(blank).collect()
    };
    panel(frame, area, format!(" {title} "), lines, true);
}

fn gauge(label: &str, value: Option<f64>, width: u16) -> Line<'static> {
    let width = usize::from(width.saturating_sub(15));
    let fill = (width as f64 * value.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0).round() as usize;
    Line::from(vec![
        Span::raw(format!(" {label:<5} ")),
        Span::styled("█".repeat(fill), style(Token::Accent)),
        Span::styled("░".repeat(width - fill), style(Token::Faint)),
        Span::styled(format!(" {:>4}", percent(value)), if value.is_some() { Style::default() } else { style(Token::Faint) }),
    ])
}

fn overview(frame: &mut Frame, app: &mut Dashboard, area: Rect, with_policy: bool, focused: bool) {
    let budget_height = if app.narrow || area.height < 11 { 5 } else { 7 };
    let [budget, lower] = Split::vertical([Constraint::Length(budget_height), Constraint::Fill(1)]).areas(area);
    let overview = match &app.detail { Some(Ok(Detail::Overview(detail))) => Some(detail), _ => None };
    let policy = overview.filter(|_| app.is_admin()).and_then(|o| o.policy.as_ref());
    memory_budget(frame, app, budget, policy, focused && with_policy);
    if with_policy && app.is_admin() {
        let areas = if app.narrow {
            Split::horizontal([Constraint::Percentage(47), Constraint::Fill(1)]).areas(lower)
        } else {
            let [policy, _, attention] = Split::horizontal([Constraint::Length(36), Constraint::Length(1), Constraint::Fill(1)]).areas(lower);
            [policy, attention]
        };
        if let Some(policy) = policy {
            let lines = if app.narrow {
                vec![
                    Line::raw(format!(" Guarantee {}G · Soft cap {}G", count(policy.mem_low_gb), count(policy.mem_high_gb))),
                    Line::raw(format!(" Total {}G · CPU weight {}", count(policy.total_max_gb), count(policy.cpu_weight))),
                    blank(" e edit policy"),
                ]
            } else { policy_lines(policy, app.data.users.len()) };
            panel(frame, areas[0], " Policy ", lines, false);
        } else {
            placeholder(frame, app, areas[0], "Policy");
        }
        attention(frame, app, areas[1], focused);
    } else if app.dashboard_home() {
        let [items, table_area] = Split::horizontal([Constraint::Fill(2), Constraint::Fill(3)]).areas(lower);
        attention(frame, app, items, app.pane == 0);
        if app.pane == 1 {
            kernels(frame, app, table_area, true, true);
        } else {
            users(frame, app, table_area, true, app.pane == 2);
        }
    } else {
        attention(frame, app, lower, focused);
    }
}

fn memory_budget(frame: &mut Frame, app: &Dashboard, area: Rect, policy: Option<&CurrentPolicy>, focused: bool) {
    let memory = app.data.system.as_ref().and_then(|s| s.mem.as_ref());
    let total = policy.and_then(|p| p.total_max_gb).map(|n| n.saturating_mul(GIB))
        .or_else(|| memory.and_then(|m| m.user_slice_max)).filter(|n| *n > 0);
    let width = usize::from(area.width.saturating_sub(4));
    let title = match total {
        Some(total) => format!(" Memory budget - user.slice max {:.0}G (WSL {}) ", total as f64 / GIB as f64, size(memory.and_then(|m| m.total))),
        None => " Memory budget ".into(),
    };
    let Some(total) = total else {
        panel(frame, area, title, vec![blank(" Memory limit n/a · r refresh")], focused);
        return;
    };
    let position = |bytes: u64| ((bytes.min(total) as u128 * width as u128) / u128::from(total)) as usize;
    let mut used = 0u64;
    let mut bar = vec![Span::raw(" ")];
    let mut labels = vec![Span::raw(" ")];
    let mut unknown = false;
    let mut segment = |name: &str, usage: Option<u64>, token| {
        unknown |= usage.is_none();
        let end = used.saturating_add(usage.unwrap_or_default());
        bar.push(Span::styled("█".repeat(position(end) - position(used)), style(token)));
        labels.push(Span::styled(format!("{} {}  ", clean(name), size(usage)), style(token)));
        used = end;
    };
    if app.is_admin() {
        for (i, user) in app.data.users.iter().enumerate() {
            segment(&user.user, user.usage.as_ref().and_then(|u| u.mem_current), [Token::Accent, Token::Secondary, Token::Info, Token::Success][i % 4]);
        }
    } else {
        let own = app.data.users.first();
        let own_usage = own.and_then(|user| user.usage.as_ref()?.mem_current);
        if let Some(user) = own {
            segment(&user.user, own_usage, Token::Accent);
        }
        labels.push(Span::styled("others n/a  ", style(Token::Faint)));
        unknown = true;
    }
    bar.push(Span::styled("░".repeat(width - position(used)), style(Token::Faint)));
    labels.push(Span::styled(format!("free {}", size((!unknown).then_some(total.saturating_sub(used)))), style(Token::Faint)));
    let guarantee = policy.and_then(|p| p.mem_low_gb);
    let guarantee_width = guarantee.map_or(0, |n| position(n.saturating_mul(GIB).saturating_mul(app.data.users.len() as u64)));
    let bracket = if guarantee_width >= 2 { format!(" └{}┘", "─".repeat(guarantee_width - 2)) } else { String::new() };
    let mut lines = vec![Line::from(bar), Line::styled(bracket, style(Token::Faint))];
    if area.height >= 7 {
        lines.push(Line::raw(format!(" {}", preview(guarantee, Some(total / GIB), app.data.users.len()))));
    }
    lines.push(Line::from(labels));
    if area.height >= 7 {
        lines.push(Line::raw(format!(" CPU share · weight {}", count(policy.and_then(|p| p.cpu_weight)))));
    }
    panel(frame, area, title, lines, focused);
}

fn attention(frame: &mut Frame, app: &Dashboard, area: Rect, focused: bool) {
    let items = app.attention();
    let title = format!(" Needs attention ({}) ", items.len());
    if items.is_empty() {
        panel(frame, area, title, vec![blank(" No items need attention · r refresh")], focused);
        return;
    }
    let selected = app.attention_row.min(items.len() - 1);
    let rows = items.iter().enumerate().map(|(i, item)| {
        let token = match item.glyph { "▲" => Token::Warn, "✗" => Token::Error, _ => Token::Info };
        Row::new([
            Cell::from(Line::from(vec![Span::styled(format!("{} ", item.glyph), data_style(token, i == selected)), Span::raw(clean(&item.text))])),
            numeric(item.time.map_or("now".into(), |time| age(app, Some(time)))),
        ]).style(selection(i == selected, focused))
    });
    let mut state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table(title, &["WHAT", "WHEN"], rows, [Constraint::Fill(1), Constraint::Length(6)], focused), area, &mut state);
}

fn table<'a>(title: impl Into<Line<'static>>, headers: &[&'static str], rows: impl IntoIterator<Item = Row<'a>>,
    widths: impl IntoIterator<Item = Constraint>, focused: bool) -> Table<'a> {
    Table::new(rows, widths).block(block(title, focused))
        .header(Row::new(headers.iter().copied()).style(style(Token::Title)))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_symbol(if focused { "❯ " } else { "  " })
}

fn numeric(value: String) -> Cell<'static> {
    let style = if value == NA { style(Token::Faint) } else { Style::default() };
    Cell::from(Line::styled(value, style).alignment(Alignment::Right))
}

fn text_cell(value: Option<&str>) -> Cell<'static> {
    let value = value.map(clean);
    match value {
        Some(value) => Cell::from(value),
        None => Cell::from(Line::styled(NA, style(Token::Faint))),
    }
}

fn visible_state(state: &TableState, indices: &[usize]) -> TableState {
    let selected = state.selected().and_then(|row| indices.iter().position(|i| *i == row));
    TableState::default().with_selected(selected).with_offset(state.offset())
}

fn memory_cells(usage: Option<&crate::api::Usage>, width: usize, selected: bool) -> Cell<'static> {
    let Some((current, low, high)) = usage.and_then(|u| Some((u.mem_current?, u.mem_low, u.mem_high?))).filter(|(_, _, h)| *h > 0)
        else { return text_cell(None); };
    // Reserve the last cell for the soft-cap marker.
    let scale = |n: u64| (n.min(high) as u128 * (width - 1) as u128 / u128::from(high)) as usize;
    let fill = scale(current).max(usize::from(current > 0));
    let guarantee = low.map(|n| scale(n).min(width - 2));
    let ramp = if current >= high { Token::Error } else if low.is_some_and(|n| current > n) { Token::Warn } else { Token::Success };
    let cells = (0..width).map(|cell| {
        let (symbol, token) = if cell == width - 1 { ("│", Token::Faint) }
            else if Some(cell) == guarantee { ("┊", if cell < fill { ramp } else { Token::Info }) }
            else if cell < fill { ("█", ramp) }
            else { ("░", Token::Faint) };
        Span::styled(symbol, data_style(token, selected))
    }).collect::<Vec<_>>();
    Cell::from(Line::from(cells))
}

fn users(frame: &mut Frame, app: &mut Dashboard, area: Rect, compact: bool, focused: bool) {
    if let Some(user) = &app.key_user {
        let title = format!(" SSH keys for {} ", clean(user));
        let Some(Ok(Detail::Keys(keys))) = &app.detail else { placeholder(frame, app, area, &title); return; };
        let indices = app.indices(Page::Users);
        if indices.is_empty() {
            table_empty(frame, area, title, "No SSH keys yet · a add key", app, focused);
            return;
        }
        let rows = indices.iter().map(|&i| {
            let key = &keys[i];
            Row::new([clean(&key.fingerprint), clean(&key.kind), clean(&key.comment)]).style(selection(app.rows.selected() == Some(i), focused))
        });
        let mut state = visible_state(&app.rows, &indices);
        frame.render_stateful_widget(table(title, &["FINGERPRINT", "TYPE", "COMMENT"], rows,
            [Constraint::Fill(4), Constraint::Fill(2), Constraint::Fill(3)], focused), area, &mut state);
        *app.rows.offset_mut() = state.offset();
        return;
    }
    let page = if compact { Page::Overview } else { Page::Users };
    let indices = app.indices(page);
    let title = if compact {
        let policy = match &app.detail { Some(Ok(Detail::Overview(o))) => o.policy.as_ref(), _ => None };
        let limits = policy.and_then(|p| p.mem_low_gb.zip(p.mem_high_gb)).or_else(|| {
            let usage = app.data.users.first()?.usage.as_ref()?;
            Some((usage.mem_low? / GIB, usage.mem_high? / GIB))
        });
        limits.map_or(" Users ".into(), |(low, high)| format!(" Users - mem vs {low}G guarantee / {high}G cap "))
    } else { format!(" Users ({}) ", app.data.users.len()) };
    if indices.is_empty() {
        let hint = if app.is_admin() { "No compute users yet · a add user" } else { "No metrics for your account · r refresh" };
        table_empty(frame, area, title, hint, app, focused);
        return;
    }
    let [list, detail] = Split::vertical([Constraint::Fill(1), Constraint::Length(if !compact && !app.narrow { 7 } else { 0 })]).areas(area);
    let show_role = !compact && app.is_admin();
    let width = if compact { usize::from(area.width.saturating_sub(35)).clamp(10, 26) } else { usize::from(area.width.saturating_sub(64)).clamp(10, 25) };
    let rows = indices.iter().map(|&i| {
        let user = &app.data.users[i];
        let usage = user.usage.as_ref();
        let selected = app.users.selected() == Some(i);
        let mut cells = vec![
            text_cell(Some(&user.user)), numeric(size(usage.and_then(|u| u.mem_current))),
            memory_cells(usage, width, selected), numeric(percent(usage.and_then(|u| u.cpu_percent))),
            numeric(count(user.kernels)),
            Cell::from(status(usage.and_then(|u| u.gpu_active), selected, "●", "·", Token::Info, Token::Faint)),
        ];
        if show_role {
            cells.insert(1, text_cell(user.role.as_deref()));
        }
        if !compact {
            cells.extend([numeric(count(user.keys)), text_cell(user.linger.map(|on| if on { "on" } else { "off" }))]);
        }
        Row::new(cells).style(selection(selected, focused))
    });
    let mut headers = vec!["USER", "   MEM", "MEMORY", " CPU", "KERN", "GPU"];
    let mut widths = vec![Constraint::Length(10), Constraint::Length(7), Constraint::Length(width as u16),
        Constraint::Length(5), Constraint::Length(4), Constraint::Length(3)];
    if show_role {
        headers.insert(1, "ROLE");
        widths.insert(1, Constraint::Length(5));
    }
    if !compact {
        headers.extend(["KEYS", "LINGER"]);
        widths.extend([Constraint::Length(4), Constraint::Length(6)]);
    }
    let mut state = visible_state(&app.users, &indices);
    frame.render_stateful_widget(table(title, &headers, rows, widths, focused), list, &mut state);
    *app.users.offset_mut() = state.offset();
    if let Some(user) = app.users.selected().and_then(|i| app.data.users.get(i)).filter(|_| detail.height > 0) {
        panel(frame, detail, format!(" {} ", clean(&user.user)), app.user_detail(user).lines().map(|s| Line::raw(clean(s))).collect(), false);
    }
}

fn table_empty(frame: &mut Frame, area: Rect, title: impl Into<Line<'static>>, hint: &'static str, app: &Dashboard, focus: bool) {
    panel(frame, area, title, vec![blank(if app.filter.is_empty() { hint } else { "No matches · esc clear filter" })], focus);
}

fn kernels(frame: &mut Frame, app: &mut Dashboard, area: Rect, compact: bool, focused: bool) {
    let indices = app.indices(Page::Kernels);
    let title = format!(" Kernels ({}) ", app.data.kernels.len());
    if indices.is_empty() {
        table_empty(frame, area, title, "No running kernels · r refresh", app, focused);
        return;
    }
    let [list, detail] = Split::vertical([Constraint::Fill(1), Constraint::Length(if compact || app.narrow { 0 } else { 4 })]).areas(area);
    let rows = indices.iter().map(|&i| {
        let kernel = &app.data.kernels[i];
        let selected = app.kernels.selected() == Some(i);
        let mut cells = vec![text_cell(Some(&kernel.id))];
        if !compact { cells.push(text_cell(Some(&kernel.user))); }
        cells.extend([text_cell(kernel.env.as_deref()), numeric(size(kernel.mem_rss))]);
        if !compact { cells.push(numeric(percent(kernel.cpu_percent))); }
        cells.push(numeric(age(app, kernel.started)));
        if !compact { cells.push(Cell::from(status(kernel.gpu_active, selected, "●", "·", Token::Info, Token::Faint))); }
        Row::new(cells).style(selection(selected, focused))
    });
    let mut state = visible_state(&app.kernels, &indices);
    let (headers, widths) = if compact {
        (vec!["ID", "ENV", "   RSS", " AGE"], vec![Constraint::Fill(2), Constraint::Fill(2), Constraint::Length(7), Constraint::Length(5)])
    } else {
        (vec!["ID", "USER", "ENV", "   RSS", " CPU", " AGE", "GPU"], vec![Constraint::Fill(3), Constraint::Fill(2),
            Constraint::Fill(2), Constraint::Length(7), Constraint::Length(5), Constraint::Length(5), Constraint::Length(3)])
    };
    frame.render_stateful_widget(table(title, &headers, rows, widths, focused), list, &mut state);
    *app.kernels.offset_mut() = state.offset();
    if let Some(kernel) = app.kernels.selected().and_then(|i| app.data.kernels.get(i)).filter(|_| detail.height > 0) {
        let description = format!(" PID {} · {}", kernel.pid, clean(kernel.kind.as_deref().unwrap_or(NA)));
        panel(frame, detail, " Connection file ", vec![Line::raw(clean(kernel.connection_file.as_deref().unwrap_or(NA))), Line::raw(description)], false);
    }
}

fn policy_lines(policy: &CurrentPolicy, users: usize) -> Vec<Line<'static>> {
    let labels = ["Guarantee", "Soft cap", "Total max", "CPU weight"];
    let mut lines = labels.into_iter().zip(POLICY_FIELDS).zip(policy_values(policy)).map(|((label, field), value)| {
        let unit = if field == "cpu_weight" || value.is_none() { "" } else { "G" };
        Line::raw(format!(" {label:<11} {}{unit}", count(value)))
    }).collect::<Vec<_>>();
    lines.push(Line::raw(""));
    lines.push(Line::raw(format!(" {}", preview(policy.mem_low_gb, policy.total_max_gb, users))));
    lines
}

fn policy(frame: &mut Frame, app: &Dashboard, area: Rect, focused: bool) {
    match &app.detail {
        Some(Ok(Detail::Policy(policy))) => panel(frame, area, " Policy ", policy_lines(policy, app.data.users.len()), focused),
        _ => placeholder(frame, app, area, "Policy"),
    }
}

fn detail_placeholder(frame: &mut Frame, app: &Dashboard, area: Rect) {
    placeholder(frame, app, area, &app.tab.label()[2..]);
}

fn envs(frame: &mut Frame, app: &mut Dashboard, area: Rect, focused: bool) {
    let Some(Ok(Detail::Envs(envs))) = &app.detail else { detail_placeholder(frame, app, area); return; };
    let indices = app.indices(Page::Envs);
    if indices.is_empty() { table_empty(frame, area, " Envs ", "No environments installed · r refresh", app, focused); return; }
    let rows = indices.iter().map(|&i| {
        let env = &envs[i];
        Row::new([text_cell(Some(&env.name)), text_cell(env.python.as_deref()), text_cell(env.spyder_kernels.as_deref()),
            text_cell(Some(&env.path))]).style(selection(app.rows.selected() == Some(i), focused))
    });
    let mut state = visible_state(&app.rows, &indices);
    frame.render_stateful_widget(table(" Envs ", &["NAME", "PYTHON", "SPYDER-KERNELS", "PATH"], rows,
        [Constraint::Fill(2), Constraint::Length(10), Constraint::Length(15), Constraint::Fill(4)], focused), area, &mut state);
    *app.rows.offset_mut() = state.offset();
}

fn audit(frame: &mut Frame, app: &mut Dashboard, area: Rect, focused: bool) {
    let Some(Ok(Detail::Audit(entries))) = &app.detail else { detail_placeholder(frame, app, area); return; };
    let indices = app.indices(Page::Audit);
    if indices.is_empty() { table_empty(frame, area, " Audit ", "No audit events yet · r refresh", app, focused); return; }
    let rows = indices.iter().map(|&i| {
        let entry = &entries[i];
        let selected = app.rows.selected() == Some(i);
        let result = status(Some(entry.ok), selected, "✓", "✗", Token::Success, Token::Error);
        Row::new([numeric(age(app, Some(entry.time))), text_cell(Some(&entry.actor)), text_cell(Some(&entry.action)),
            text_cell(Some(&entry.target)), Cell::from(result), text_cell(entry.detail.as_deref())]).style(selection(selected, focused))
    });
    let mut state = visible_state(&app.rows, &indices);
    frame.render_stateful_widget(table(" Audit ", &["TIME", "ACTOR", "ACTION", "TARGET", "RESULT", "DETAIL"], rows,
        [Constraint::Length(5), Constraint::Fill(2), Constraint::Fill(3), Constraint::Fill(3), Constraint::Length(6), Constraint::Fill(4)], focused),
        area, &mut state);
    *app.rows.offset_mut() = state.offset();
}

fn hardware(frame: &mut Frame, app: &Dashboard, area: Rect, focused: bool) {
    let [hardware, npu] = Split::vertical([Constraint::Fill(1), Constraint::Length(live_ui::NPU_DETAIL_HEIGHT)]).areas(area);
    live_ui::npu_panel(frame, app, npu, true);
    let Some(Ok(Detail::Hardware(hw))) = &app.detail else { detail_placeholder(frame, app, hardware); return; };
    let storage_height = if hardware.height < 18 { 5 } else { 8 };
    let [top, storage] = Split::vertical([Constraint::Fill(1), Constraint::Length(storage_height)]).areas(hardware);
    let [compute, _, host] = Split::horizontal([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).areas(top);
    let Hardware {cpu, memory, gpus, board, os, host_boot, disks} = hw;
    let dimms = memory.as_ref().and_then(|m| m.dimms.as_ref()).map_or(NA.into(), |dimms| {
        if let Some(items) = dimms.as_array().filter(|items| !items.is_empty()) {
            if let Some(capacity) = items[0].as_u64().filter(|n| items.iter().all(|item| item.as_u64() == Some(*n))) {
                return format!("{} x {}", items.len(), size(Some(capacity)));
            }
        }
        clean(dimms.as_str().map(str::to_owned).unwrap_or_else(|| dimms.to_string()).as_str())
    });
    let mut metrics = vec![
        Line::raw(format!(" CPU      {}", clean(cpu.as_ref().and_then(|c| c.model.as_deref()).unwrap_or(NA)))),
        Line::raw(format!(" Cores    {} (WSL may use {})", count(cpu.as_ref().and_then(|c| c.logical)), count(cpu.as_ref().and_then(|c| c.wsl_processors)))),
        Line::raw(format!(" RAM host {}  {}", size(memory.as_ref().and_then(|m| m.host_total)), dimms)),
        Line::raw(format!(" Speed    {} MT/s", count(memory.as_ref().and_then(|m| m.speed_mts)))),
        Line::raw(format!(" RAM WSL  {}  (user.slice max {})", size(memory.as_ref().and_then(|m| m.wsl_total)), size(memory.as_ref().and_then(|m| m.user_slice_max)))),
    ];
    for gpu in gpus.iter().flatten() {
        metrics.push(Line::raw(format!(" GPU {}    {}", count(gpu.index), clean(gpu.name.as_deref().unwrap_or(NA)))));
        metrics.push(Line::raw(format!(" VRAM     {}", size(gpu.mem_total))));
    }
    panel(frame, compute, " Compute ", metrics, focused);
    let boot = host_boot.as_ref().map_or(NA.into(), |boot| boot.as_str().map(str::to_owned).unwrap_or_else(|| boot.to_string()));
    panel(frame, host, " Host ", vec![
        Line::raw(format!(" Board   {}", clean(board.as_deref().unwrap_or(NA)))),
        Line::raw(format!(" OS      {}", clean(os.as_deref().unwrap_or(NA)))),
        Line::raw(format!(" Boot    {}", clean(&boot))),
    ], false);
    let mut volumes = Vec::new();
    if let Some(disks) = disks {
        for disk in disks.physical.iter().flatten() {
            volumes.push(Line::raw(format!(" {} ({})  {}  physical disk", clean(disk.name.as_deref().unwrap_or(NA)),
                clean(disk.media.as_deref().unwrap_or(NA)), size(disk.size))));
        }
        for volume in disks.linux.iter().flatten().chain(disks.windows.iter().flatten()) {
            let mount = match volume.mount.as_deref() {
                Some("/mnt/c") => "C:",
                Some(mount) => mount,
                None => NA,
            };
            let label = format!("{:<4} {:>8}", clean(mount), size(volume.total));
            volumes.push(gauge(&label, ratio(volume.used, volume.total), storage.width.saturating_sub(8)));
        }
    }
    if volumes.is_empty() { volumes.push(blank(" No storage data · r refresh")); }
    panel(frame, storage, " Storage ", volumes, false);
}

fn wsl(frame: &mut Frame, app: &Dashboard, area: Rect, focused: bool) {
    let Some(Ok(Detail::Wsl(wsl, _))) = &app.detail else { detail_placeholder(frame, app, area); return; };
    let Wsl {wsl_version, kernel, windows, distros, autostart_task, config, restart_pending} = wsl;
    let distro_rows = distros.as_ref().map_or(1, |items| items.len().max(1));
    let height = (distro_rows + 5).clamp(8, usize::from(area.height.saturating_sub(9).max(8)));
    let [status_area, config_area] = Split::vertical([Constraint::Length(height as u16), Constraint::Fill(1)]).areas(area);
    let string = |v: &Option<String>| clean(v.as_deref().unwrap_or(NA));
    let mut lines = vec![
        Line::raw(format!(" Version   WSL {}  kernel {}", string(wsl_version), string(kernel))),
        Line::raw(format!(" Windows   {}", string(windows))),
    ];
    for distro in distros.iter().flatten().take(height.saturating_sub(5)) {
        lines.push(Line::from(vec![
            Span::raw(" Distro    "),
            status(distro.state.as_ref().map(|s| s == "Running"), false, "✓", "·", Token::Success, Token::Faint),
            Span::raw(format!(" {}  {}  v{}{}", string(&distro.name), string(&distro.state),
                    count(distro.version), if distro.default == Some(true) { "  default" } else { "" })),
        ]));
    }
    if distros.as_ref().is_none_or(Vec::is_empty) { lines.push(blank(" No WSL distros reported · r refresh")); }
    lines.push(Line::from(vec![
        Span::raw(" Autostart "),
        status(autostart_task.as_ref().map(|state| state != "missing"), false, "✓", "▲", Token::Success, Token::Warn),
        Span::raw(format!(" task WSL-Autostart · {}", string(autostart_task))),
    ]));
    panel(frame, status_area, " WSL ", lines, false);
    let mut lines = if let Some(config) = config {
        vec![
            Line::raw(format!(" memory       {} GiB", count(config.memory_gb))),
            Line::raw(format!(" processors   {}", count(config.processors))),
            Line::raw(format!(" swap         {} GiB", count(config.swap_gb))),
            Line::raw(format!(" networking   {}", string(&config.networking_mode))),
            Line::raw(format!(" idle timeout {}", config.vm_idle_timeout.map_or(NA.into(), |n| if n == -1 { "-1 (never stop)".into() } else { format!("{n} ms") }))),
        ]
    } else { vec![blank(" No .wslconfig · e edit")] };
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![Span::raw(" state        "),
        status(*restart_pending, false, "▲ restart pending", "✓ in effect", Token::Warn, Token::Success)]));
    panel(frame, config_area, format!(" .wslconfig  {} ", config.as_ref().and_then(|c| c.path.as_deref()).map(clean).unwrap_or_default()), lines, focused);
}

fn history_segments(points: &[(f64, Option<f64>)]) -> Vec<Vec<(f64, f64)>> {
    points.split(|(_, value)| !value.is_some_and(f64::is_finite))
        .filter(|run| !run.is_empty())
        .map(|run| run.iter().filter_map(|(time, value)| value.map(|value| (*time, value))).collect())
        .collect()
}

fn history(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let [filter, grid] = Split::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    let (range, seconds, _) = RANGES[app.range];
    let user = app.history_user.as_deref().unwrap_or(NA);
    frame.render_widget(Paragraph::new(format!("Range {range} · User {user} · {} metrics", METRICS.len())), filter);
    let Some(Ok(Detail::History(all, board))) = &app.detail else {
        detail_placeholder(frame, app, grid);
        return;
    };
    let board_height = if board.is_empty() { 0 } else { board.len().min(8) as u16 + 3 };
    let [board_area, grid] = Split::vertical([Constraint::Length(board_height), Constraint::Fill(1)]).areas(grid);
    if !board.is_empty() {
        leaderboard(frame, board, range, board_area);
    }
    let cols = if grid.width >= 150 { 4 } else if grid.width >= 90 { 3 } else { 2 };
    let rows = all.len().div_ceil(cols);
    let row_areas = Split::vertical(vec![Constraint::Fill(1); rows]).split(grid);
    for (index, (metric, result)) in all.iter().enumerate() {
        let cell = Split::horizontal(vec![Constraint::Fill(1); cols]).split(row_areas[index / cols])[index % cols];
        history_chart(frame, metric, result, seconds, range, index == app.metric, cell);
    }
}

fn leaderboard(frame: &mut Frame, board: &[crate::api::UsageRank], range: &str, area: Rect) {
    let top_cpu = board.first().map_or(1.0, |u| u.cpu_avg).max(1.0);
    let mem_king = board.iter().max_by(|a, b| a.mem_peak.total_cmp(&b.mem_peak)).map(|u| u.user.clone());
    let width = usize::from(area.width.saturating_sub(70)).clamp(4, 40);
    let mut lines = vec![Line::styled(format!(" {:<4} {:<14} {:<w$} {:>8} {:>8} {:>10} {:>10}", "#", "user", "", "CPU avg", "peak", "mem avg", "mem peak", w = width), style(Token::Faint))];
    for (i, u) in board.iter().take(8).enumerate() {
        let medal = match i { 0 => "🥇", 1 => "🥈", 2 => "🥉", _ => "  " };
        let filled = ((u.cpu_avg / top_cpu) * width as f64).round() as usize;
        let bar: String = "█".repeat(filled.min(width)) + &"·".repeat(width - filled.min(width));
        let crown = if mem_king.as_deref() == Some(u.user.as_str()) && u.mem_peak > 0.0 { " 🐘" } else { "" };
        lines.push(Line::from(vec![
            Span::raw(format!(" {medal}{:<2} {:<14} ", i + 1, clean(&u.user))),
            Span::styled(bar, style(if i == 0 { Token::Warn } else { Token::Info })),
            Span::raw(format!(" {:>8} {:>8} {:>10} {:>10}{crown}", percent(Some(u.cpu_avg)), percent(Some(u.cpu_peak)),
                bytes(Some(u.mem_avg as u64)), bytes(Some(u.mem_peak as u64)))),
        ]));
    }
    frame.render_widget(Paragraph::new(lines).block(block(format!("Leaderboard · {range} · 🐘 memory champion"), false)), area);
}

fn history_chart(frame: &mut Frame, metric: &str, result: &Result<crate::api::History, String>, seconds: u64, range: &str, focused: bool, area: Rect) {
    let memory = matches!(metric, "mem_used" | "gpu_mem_used" | "user_mem");
    let rate = metric.starts_with("net_") || metric.starts_with("disk_");
    let unit = |value: f64| if memory { bytes(Some(value as u64)) }
        else if rate { format!("{}/s", bytes(Some(value as u64))) }
        else if metric == "gpu_temp" { format!("{value:.0}°C") }
        else { percent(Some(value)) };
    let history = match result {
        Ok(history) => history,
        Err(error) => {
            frame.render_widget(Paragraph::new(clean(error)).style(style(Token::Faint)).block(block(metric.to_string(), focused)), area);
            return;
        }
    };
    let segments = history_segments(&history.points);
    let latest_value = segments.iter().flatten().last().map(|(_, y)| *y);
    let title = format!("{metric}  {}", latest_value.map(unit).unwrap_or_else(|| NA.into()));
    if segments.is_empty() {
        frame.render_widget(Paragraph::new("No samples").style(style(Token::Faint)).block(block(title.clone(), focused)), area);
        return;
    }
    let latest = history.points.last().map_or(0.0, |(time, _)| *time);
    let [start, end] = history.window.unwrap_or([latest - seconds as f64, latest]);
    let floor = if memory || rate { 1.0 } else { 100.0 };
    let max = segments.iter().flatten().map(|(_, y)| *y).fold(floor, f64::max);
    let series = segments.iter().map(|segment| Dataset::default()
        .marker(Marker::Braille)
        .graph_type(if segment.len() == 1 { GraphType::Scatter } else { GraphType::Line })
        .style(style(Token::Info)).data(segment)).collect();
    frame.render_widget(Chart::new(series).block(block(title.clone(), focused))
        .x_axis(Axis::default().bounds([start, end]).labels([format!("-{range}"), "now".into()]))
        .y_axis(Axis::default().bounds([0.0, max]).labels([unit(0.0), unit(max)])), area);
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace().map(clean) {
        let line = lines.last_mut().unwrap();
        if line.is_empty() {
            *line = word;
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(&word);
        } else {
            lines.push(word);
        }
    }
    lines
}

fn draw_modal(frame: &mut Frame, app: &Dashboard, modal: &Modal) {
    let (title, lines, height, wrap) = match modal {
        Modal::Palette { query, selected } => {
            let actions = app.actions(query);
            let offset = selected.saturating_sub(11);
            let mut lines = vec![Line::raw(format!(": {}", clean(query))), Line::raw("")];
            if actions.is_empty() {
                lines.push(Line::styled("No matching commands · esc close", style(Token::Faint)));
            }
            lines.extend(actions.iter().enumerate().skip(offset).take(12).map(|(i, action)| {
                let (prefix, style) = if i == *selected { ("❯", style(Token::Selection)) } else { (" ", Style::default()) };
                Line::styled(format!("{prefix} {:<10} {}", action.key, action.name), style)
            }));
            ("Commands", lines, 17, false)
        }
        Modal::Info { title, text, .. } => (title.as_str(),
            text.lines().map(|line| Line::raw(clean(line))).chain([Line::raw(""), Line::raw("Enter / Esc close")]).collect(),
            19, true),
        Modal::Confirm { prompt, action } => {
            let hint = if matches!(action, crate::api::Action::SetUserRole { .. }) { "y/N · " } else { "y confirm · " };
            ("Confirm", vec![
                Line::raw(clean(prompt)),
                Line::raw(""),
                Line::from(vec![Span::raw(hint), Span::styled("[Cancel]", style(Token::Selection)), Span::raw(" esc / enter")]),
            ], 8, true)
        },
        Modal::Form(form) => {
            let width = usize::from(frame.area().width.min(72).saturating_sub(4));
            let prompt = form.prompt();
            let mut lines = Vec::new();
            for line in wrap_words(&prompt, width) {
                lines.push(Line::raw(line));
            }
            let prompt_height = lines.len() as u16;
            for (index, (label, value)) in form.fields.iter().enumerate() {
                let selected = index == form.selected && !form.cancel_focused();
                if selected {
                    let value_before = &value[..form.cursor];
                    let mut before: Vec<_> = value_before.chars().rev().take(width.saturating_sub(label.len() + 4)).collect();
                    before.reverse();
                    let tail = &value[form.cursor..];
                    let cursor = tail.chars().next().unwrap_or(' ');
                    let rendered = |ch| if ch == '\n' || ch == '\r' { '↵' } else { glyph(ch) };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{label}  "), style(Token::Info)),
                        Span::raw(before.into_iter().map(rendered).collect::<String>()),
                        Span::styled(rendered(cursor).to_string(), Style::default().add_modifier(Modifier::REVERSED)),
                        Span::raw(tail.chars().skip(1).map(rendered).collect::<String>()),
                    ]));
                } else {
                    lines.push(Line::raw(format!("{label}  {}", clean(&value.replace(['\n', '\r'], "↵")))));
                }
            }
            lines.push(Line::styled(form.error.as_deref().unwrap_or_default(), style(Token::Error)));
            lines.push(match form.kind {
                crate::app::FormKind::Restart | crate::app::FormKind::Disable(_) => Line::from(vec![
                    Span::raw(if form.cancel_focused() { "Type to confirm · " } else { "Enter submit · " }),
                    Span::styled("[Cancel]", selection(form.cancel_focused(), true)),
                    Span::raw(" esc"),
                ]),
                _ => Line::raw("Tab / Shift+Tab fields · Enter submit · Esc cancel"),
            });
            (form.title(), lines, 5 + prompt_height + form.fields.len() as u16, false)
        }
    };
    let area = frame.area();
    let width = area.width.min(72);
    let height = area.height.min(height);
    let popup = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height);
    frame.render_widget(Clear, popup);
    let mut widget = Paragraph::new(lines).block(block(format!(" {} ", clean(title)), true));
    if wrap {
        widget = widget.wrap(Wrap { trim: false });
    }
    if let Modal::Info { scroll, .. } = modal {
        widget = widget.scroll((*scroll, 0));
    }
    frame.render_widget(widget, popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::{Identity, Snapshot, Source}, fixture_app};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    fn source() -> Source {
        Source::Mock(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"))
    }

    fn render(app: &mut Dashboard, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn plain(buffer: &Buffer) -> String {
        buffer.content.chunks(usize::from(buffer.area.width)).map(|row| {
            row.iter().map(|cell| cell.symbol()).collect::<String>()
        }).collect::<Vec<_>>().join("\n")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn npu_status_fits_overview_and_hardware_views() {
        for (width, height) in [(80, 24), (120, 30)] {
            for page in [Page::Overview, Page::Hardware] {
                let mut app = fixture_app(&source(), Layout::Console, page).await;
                for live in [false, true] {
                    app.hardware_live = live;
                    let output = plain(&render(&mut app, width, height));
                    for label in ["Intel(R) AI Boost", "✓ up", "driver 1004404", "active alice", "queued 2", "served 14", "avg 0.05s"] {
                        assert!(output.contains(label), "missing {label}: {output}");
                    }
                    if live || page == Page::Hardware {
                        assert!(output.contains("Devices CPU, GPU.0, NPU") && output.contains("OpenVINO 2025.4"), "{output}");
                    }
                }
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn npu_worker_down_is_red() {
        let mut app = fixture_app(&source(), Layout::Console, Page::Overview).await;
        app.data.npu = Some(crate::api::NpuStatus { up: Some(false), ..Default::default() });
        let buffer = render(&mut app, 80, 24);
        let output = plain(&buffer);
        let row = output.lines().position(|line| line.contains("✗ NPU worker down")).unwrap();
        if crate::theme::colour_enabled() {
            assert_eq!(buffer.content[row * 80 + 1].fg, ratatui::style::Color::Red);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn role_confirmation_defaults_to_no_and_can_grant_or_revoke() {
        let mut app = fixture_app(&source(), Layout::Console, Page::Users).await;
        app.users.select(Some(1));
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        app.key(key(KeyCode::Char('A')));
        let output = plain(&render(&mut app, 120, 30));
        assert!(output.contains("Make bob an admin? They can manage users, policy and WSL"));
        assert!(output.contains("y/N"));
        assert!(matches!(app.key(key(KeyCode::Enter)), crate::app::Input::None));
        assert!(app.modal.is_none());
        app.key(key(KeyCode::Char('A')));
        let crate::app::Input::Command(crate::app::Command::Mutate(action)) = app.key(key(KeyCode::Char('y'))) else {
            panic!("expected role mutation");
        };
        assert_eq!(action, crate::api::Action::SetUserRole { user: "bob".into(), role: crate::api::UserRole::Admin });

        let source = source();
        let http = crate::api::client();
        let mut mock = crate::api::MockState::default();
        let stream = crate::api::StreamMetrics::default();
        source.current(&http, &mut mock, &stream).await;
        source.mutate(&http, &mut mock, action).await.unwrap();
        app.update(source.current(&http, &mut mock, &stream).await);
        app.busy = false;
        assert!(plain(&render(&mut app, 120, 30)).lines().any(|line| line.contains("bob") && line.contains("admin")));
        app.key(key(KeyCode::Char('A')));
        assert!(matches!(app.key(key(KeyCode::Char('y'))),
            crate::app::Input::Command(crate::app::Command::Mutate(crate::api::Action::SetUserRole {
                user, role: crate::api::UserRole::User
            })) if user == "bob"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn renders_all_pages_in_all_layouts_at_all_sizes() {
        for layout in Layout::ALL {
            for (page, expected) in Page::ALL.into_iter().zip([
                "Memory budget",
                "LINGER", "RSS", "Guaranteed", "CT2000T500SSD8", "Ubuntu-24.04",
                "SPYDER-KERNELS", "cpu_percent", "RESULT",
            ]) {
                let mut app = fixture_app(&source(), layout, page).await;
                assert!(app.data.errors.is_empty(), "{:?}", app.data.errors);
                assert!(app.detail_error.is_none(), "{:?}", app.detail_error);
                for (width, height) in [(80, 24), (120, 30), (160, 45)] {
                    let output = plain(&render(&mut app, width, height));
                    assert!(output.contains(expected), "{layout:?} {} {width}x{height}: missing {expected}\n{output}", page.label());
                    if page == Page::Overview {
                        for label in ["CPU", "MEM", "GPU", "Needs attention"] {
                            assert!(output.contains(label), "{layout:?} {width}x{height}: {output}");
                        }
                        if width >= 120 {
                            for label in ["NET", "DISK", "C23", "C:"] {
                                assert!(output.contains(label), "{layout:?} {width}x{height}: {output}");
                            }
                        } else {
                            assert!(output.contains("24 cores") && !output.contains("C23"), "{output}");
                        }
                        assert!(output.chars().any(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch)), "{output}");
                    }
                }
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hardware_live_opens_from_both_pages_and_returns_to_origin() {
        for layout in Layout::ALL {
            for (page, key) in [(Page::Overview, 'h'), (Page::Hardware, 'l')] {
                let mut app = fixture_app(&source(), layout, page).await;
                app.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
                for (width, height) in [(80, 24), (120, 30), (160, 45)] {
                    let output = plain(&render(&mut app, width, height));
                    assert!(output.contains("Hardware live"), "{output}");
                    for label in ["CPU", "MEM", "GPU", "NET", "DISK", "C23", "esc back"] {
                        assert!(output.contains(label), "{output}");
                    }
                    assert!(!output.contains("Memory budget"), "{output}");
                }
                app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                assert!(app.tab == page);
                assert!(!app.hardware_live);
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn role_gating_hides_admin_pages_in_navigation_and_palette() {
        for layout in Layout::ALL {
            let mut app = fixture_app(&source(), layout, Page::Overview).await;
            app.update(Snapshot { identity: Some(Identity { user: "alice".into(), role: "user".into() }), ..Snapshot::default() });
            assert!(app.actions("Toggle admin role").is_empty());
            app.key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
            assert!(matches!(&app.modal, Some(Modal::Info { text, .. }) if !text.contains("Toggle admin role")));
            app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            let output = plain(&render(&mut app, 120, 30));
            for hidden in ["2 Users", "4 Policy", "6 WSL", "9 Audit"] {
                assert!(!output.contains(hidden));
            }
            for number in ['2', '4', '6', '9'] {
                app.key(KeyEvent::new(KeyCode::Char(number), KeyModifiers::NONE));
                assert!(app.tab == Page::Overview);
            }
            assert!(app.actions("").iter().all(|action| action.page.is_none_or(|page| page.visible(false))));
        }
    }

    #[test]
    fn minimum_checks_both_dimensions() {
        let mut app = Dashboard::default();
        for (width, height) in [(70, 20), (79, 24), (80, 23)] {
            let output = plain(&render(&mut app, width, height));
            assert!(output.contains(&format!("cm-top needs 80×24, window is {width}×{height} · q quit")));
        }
        let output = plain(&render(&mut app, 80, 24));
        assert!(!output.contains("cm-top needs"));
        assert!(output.contains("████████"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_frame_keeps_previous_rows_and_retry_hint() {
        let mut app = fixture_app(&source(), Layout::Dashboard, Page::Overview).await;
        app.update(Snapshot {
            errors: vec!["/system: /run/compute-mgr/api.sock: connection refused".into()],
            ..Snapshot::default()
        });
        let buffer = render(&mut app, 120, 30);
        let output = plain(&buffer);
        assert!(output.contains("STALE "));
        assert!(output.contains("✗ daemon unreachable (/run/compute-mgr/api.sock) · r retry"));
        assert!(output.contains("alice"));
        let i = buffer.content.iter().position(|cell| cell.symbol() == "┊").unwrap();
        assert!(buffer.content[i].modifier.contains(Modifier::DIM));
        app.update(fixture_app(&source(), Layout::Console, Page::Overview).await.data);
        assert!(!plain(&render(&mut app, 120, 30)).contains("STALE "));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn selected_row_keeps_memory_and_gpu_colours() {
        let mut app = fixture_app(&source(), Layout::Console, Page::Users).await;
        app.users.select(Some(0));
        let selected = render(&mut app, 120, 30);
        app.users.select(Some(1));
        let unselected = render(&mut app, 120, 30);
        for symbol in ["┊", "●"] {
            let cell = selected.content.iter().position(|cell| cell.symbol() == symbol).unwrap();
            assert_eq!(unselected.content[cell].symbol(), symbol);
            assert_eq!(selected.content[cell].fg, unselected.content[cell].fg);
            if crate::theme::colour_enabled() {
                assert_ne!(selected.content[cell].fg, ratatui::style::Color::Reset);
                assert!(!selected.content[cell].modifier.contains(Modifier::REVERSED));
                assert_ne!(selected.content[cell].bg, unselected.content[cell].bg);
            }
        }
    }

    #[test]
    fn history_keeps_null_gaps_and_single_samples() {
        let points = [(0.0, None), (1.0, Some(0.0)), (2.0, None), (3.0, Some(5.0)), (4.0, Some(7.0)), (5.0, None)];
        assert_eq!(history_segments(&points), vec![vec![(1.0, 0.0)], vec![(3.0, 5.0), (4.0, 7.0)]]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retains_keys_view_fixture() {
        let source = source();
        let mut app = fixture_app(&source, Layout::Console, Page::Users).await;
        assert!(matches!(app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), crate::app::Input::Command(_)));
        let view = app.view();
        let detail = source.detail(&crate::api::client(), &mut crate::api::MockState::default(), &view, app.data.identity.as_ref()).await;
        app.update_detail(view, detail);
        assert!(plain(&render(&mut app, 120, 30)).contains("fixture-alice"));
    }
}
