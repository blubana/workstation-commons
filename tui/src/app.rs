use crate::api::{admin, can_stop, Action, CurrentPolicy, Detail, Policy, Snapshot, UserRole, View, WslEdit, METRICS, RANGES};
use crate::{config::Layout, live_history::HardwareHistory};
use std::{collections::HashMap, path::PathBuf, time::{Duration, Instant}};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    Users,
    Kernels,
    Policy,
    Hardware,
    Wsl,
    Envs,
    History,
    Audit,
}

impl Page {
    pub const ALL: [Self; 9] = [Self::Overview, Self::Users, Self::Kernels, Self::Policy, Self::Hardware, Self::Wsl, Self::Envs, Self::History, Self::Audit];

    pub fn visible(self, is_admin: bool) -> bool {
        is_admin || !matches!(self, Self::Users | Self::Policy | Self::Wsl | Self::Audit)
    }

    pub fn label(self) -> &'static str {
        ["1 Overview", "2 Users", "3 Kernels", "4 Policy", "5 Hardware", "6 WSL", "7 Envs", "8 History", "9 Audit"][self as usize]
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.label()[2..].eq_ignore_ascii_case(value))
    }

    pub fn filterable(self) -> bool {
        matches!(self, Self::Overview | Self::Users | Self::Kernels | Self::Envs | Self::Audit)
    }
}

pub enum Command {
    Load(View),
    Refresh(View),
    Mutate(Action),
}

pub enum Input {
    None,
    Quit,
    Launch(&'static str),
    Command(Command),
}

pub enum Modal {
    Info { title: String, text: String, scroll: u16 },
    Confirm { prompt: String, action: Action },
    Form(Form),
    Palette { query: String, selected: usize },
}

pub enum FormKind {
    AddUser,
    Disable(String),
    AddKey(String),
    Policy,
    Wsl(u64),
    Restart,
}

pub const POLICY_FIELDS: [&str; 4] = ["mem_low_gb", "mem_high_gb", "total_max_gb", "cpu_weight"];

pub struct Form {
    pub kind: FormKind,
    pub fields: Vec<(&'static str, String)>,
    pub selected: usize,
    pub cursor: usize,
    pub error: Option<String>,
    pub user_count: usize,
}

impl Form {
    fn new(kind: FormKind, fields: Vec<(&'static str, String)>) -> Self {
        let cursor = fields.first().map_or(0, |(_, value)| value.len());
        Self { kind, fields, selected: 0, cursor, error: None, user_count: 0 }
    }

    pub fn title(&self) -> &str {
        match &self.kind {
            FormKind::AddUser => "Add user",
            FormKind::Disable(_) => "Disable user",
            FormKind::AddKey(_) => "Add SSH public key",
            FormKind::Policy => "Edit policy",
            FormKind::Wsl(_) => "Edit WSL configuration",
            FormKind::Restart => "Restart WSL?",
        }
    }

    pub fn prompt(&self) -> String {
        match &self.kind {
            FormKind::Disable(user) => format!("Type {user} to disable the account and end its sessions."),
            FormKind::AddKey(user) => format!("Add an SSH public key for {user}."),
            FormKind::Restart => "▲ Disconnects everyone and stops every kernel. WSL returns in about 90 s. Type restart to confirm.".into(),
            FormKind::Wsl(cores) => format!("Memory 16..124 GiB · processors 2..{cores} · swap 0..64 GiB"),
            FormKind::Policy => match self.submit() {
                Ok(Action::SetPolicy(policy)) => preview(Some(policy.mem_low_gb), Some(policy.total_max_gb), self.user_count),
                _ => "Memory must satisfy low ≤ high ≤ total".into(),
            },
            _ => String::new(),
        }
    }

    pub fn paste(&mut self, text: &str) {
        let value = &mut self.fields[self.selected].1;
        let text: String = text.trim_end_matches(['\r', '\n']).chars()
            .filter(|ch| !ch.is_control() || *ch == '\r' || *ch == '\n').collect();
        if value.len() + text.len() <= 16384 {
            value.insert_str(self.cursor, &text);
            self.cursor += text.len();
            self.error = None;
        }
    }

    fn edit(&mut self, key: KeyEvent) {
        let value = &mut self.fields[self.selected].1;
        let previous = value[..self.cursor].char_indices().next_back().map(|(pos, _)| pos);
        let next = value[self.cursor..].chars().next().map(|ch| self.cursor + ch.len_utf8());
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.focus(true),
            KeyCode::BackTab | KeyCode::Up => self.focus(false),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = value.len(),
            KeyCode::Left => self.cursor = previous.unwrap_or(self.cursor),
            KeyCode::Right => self.cursor = next.unwrap_or(self.cursor),
            KeyCode::Backspace => {
                if let Some(start) = previous {
                    value.drain(start..self.cursor);
                    self.cursor = start;
                }
            }
            KeyCode::Delete => {
                if let Some(end) = next {
                    value.drain(self.cursor..end);
                }
            }
            KeyCode::Char(ch) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.paste(ch.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
        self.error = None;
    }

    fn focus(&mut self, forward: bool) {
        self.selected = cycle(self.selected, self.fields.len(), forward);
        self.cursor = self.fields[self.selected].1.len();
    }

    pub fn cancel_focused(&self) -> bool {
        matches!(self.kind, FormKind::Disable(_) | FormKind::Restart) && self.fields[0].1.is_empty()
    }

    fn submit(&self) -> Result<Action, String> {
        let value = |index: usize| self.fields[index].1.as_str();
        match &self.kind {
            FormKind::AddUser => {
                let user = value(0);
                if !valid_username(user) {
                    return Err("Username must match [a-z_][a-z0-9_-]{0,30}".into());
                }
                Ok(Action::AddUser { user: user.into(), pubkey: public_key(value(1))? })
            }
            FormKind::AddKey(user) => Ok(Action::AddKey { user: user.clone(), pubkey: public_key(value(0))? }),
            FormKind::Disable(user) if user == value(0) => Ok(Action::Disable(user.clone())),
            FormKind::Disable(_) => Err("Username does not match".into()),
            FormKind::Policy => Ok(Action::SetPolicy(parse_policy(std::array::from_fn(value))?)),
            FormKind::Wsl(cores) => Ok(Action::SetWsl(parse_wsl(std::array::from_fn(value), *cores)?)),
            FormKind::Restart if value(0) == "restart" => Ok(Action::RestartWsl),
            FormKind::Restart => Err("Type restart to confirm".into()),
        }
    }
}

fn valid_username(user: &str) -> bool {
    let mut bytes = user.bytes();
    user.len() <= 31 && bytes.next().is_some_and(|ch| ch.is_ascii_lowercase() || ch == b'_')
        && bytes.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, b'_' | b'-'))
}

fn public_key(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.contains(['\r', '\n']) || value.split_whitespace().count() < 2 {
        return Err("Paste one SSH public key".into());
    }
    Ok(value.into())
}

fn parse_policy(values: [&str; 4]) -> Result<Policy, String> {
    let mut parsed = [0; 4];
    for ((target, value), field) in parsed.iter_mut().zip(values).zip(POLICY_FIELDS) {
        *target = value.trim().parse().map_err(|_| format!("{field} must be a nonnegative integer"))?;
    }
    let [mem_low_gb, mem_high_gb, total_max_gb, cpu_weight] = parsed;
    if mem_low_gb > mem_high_gb || mem_high_gb > total_max_gb {
        return Err("Memory limits must satisfy low ≤ high ≤ total".into());
    }
    Ok(Policy { mem_low_gb, mem_high_gb, total_max_gb, cpu_weight })
}

fn parse_wsl(values: [&str; 3], cores: u64) -> Result<WslEdit, String> {
    let mut numbers = [0; 3];
    for (((number, value), name), (min, max)) in numbers.iter_mut().zip(values)
        .zip(["Memory", "Processors", "Swap"]).zip([(16, 124), (2, cores), (0, 64)])
    {
        *number = value.trim().parse().ok().filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("{name} must be an integer in {min}..{max}"))?;
    }
    let [memory_gb, processors, swap_gb] = numbers;
    Ok(WslEdit { memory_gb, processors, swap_gb })
}

pub fn preview(low: Option<u64>, max: Option<u64>, users: usize) -> String {
    match (low, max) {
        (Some(low), Some(max)) => {
            let guaranteed = u128::from(low) * users as u128;
            let warning = if guaranteed > u128::from(max) { " · ▲ over budget" } else { "" };
            format!("Guaranteed {users} x {low}G = {guaranteed}G of {max}G{warning}")
        }
        _ => "Guaranteed memory n/a".into(),
    }
}

pub fn fuzzy(query: &str, text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    query.to_ascii_lowercase().split_whitespace().all(|word| {
        let mut chars = text.chars();
        word.chars().all(|target| chars.by_ref().any(|ch| ch == target))
    })
}

pub fn policy_values(policy: &CurrentPolicy) -> [Option<u64>; 4] {
    [policy.mem_low_gb, policy.mem_high_gb, policy.total_max_gb, policy.cpu_weight]
}

pub struct IdleObservation {
    started: Option<f64>,
    first: Instant,
    last: Instant,
}

#[derive(Default)]
pub struct Dashboard {
    pub data: Snapshot,
    pub hardware_history: HardwareHistory,
    pub hardware_live: bool,
    pub tab: Page,
    pub layout: Layout,
    pub dashboard_page: bool,
    pub state_path: Option<PathBuf>,
    pub hosts: Vec<String>,
    pub filter: String,
    pub filtering: bool,
    pub narrow: bool,
    pub too_small: bool,
    pub pane: usize,
    pub attention_row: usize,
    pub idle_kernels: HashMap<String, IdleObservation>,
    pub restarted: Option<Instant>,
    pub stale: Option<Instant>,
    pub detail_error: Option<String>,
    pub deferred: Option<(View, KeyCode)>,
    pub users: TableState,
    pub kernels: TableState,
    pub rows: TableState,
    pub key_user: Option<String>,
    pub metric: usize,
    pub range: usize,
    pub history_user: Option<String>,
    pub detail: Option<Result<Detail, String>>,
    pub modal: Option<Modal>,
    pub busy: bool,
    pub busy_since: Option<Instant>,
    pub status: Option<Result<String, String>>,
    pub loaded: bool,
}

impl Dashboard {
    pub fn is_admin(&self) -> bool {
        admin(self.data.identity.as_ref())
    }

    pub fn update(&mut self, data: Snapshot) {
        if !data.errors.is_empty() {
            self.stale.get_or_insert_with(Instant::now);
            if self.loaded && data.identity.is_none() {
                self.data.errors = data.errors;
                return;
            }
        } else if self.detail_error.is_none() {
            self.stale = None;
        }
        self.hardware_history.update(&data.hardware_history, &data.stream_samples, data.system.as_ref());
        let mut data = data;
        if self.loaded && self.data.identity.as_ref().zip(data.identity.as_ref())
            .is_some_and(|(old, new)| old.user == new.user && old.role == new.role)
        {
            if data.system.is_none() { data.system = self.data.system.clone(); }
            if data.errors.iter().any(|error| error.starts_with("/users:")) { data.users = self.data.users.clone(); }
            if data.errors.iter().any(|error| error.starts_with("/kernels:")) { data.kernels = self.data.kernels.clone(); }
        }
        let user = self.users.selected().and_then(|i| self.data.users.get(i));
        let kernel = self.kernels.selected().and_then(|i| self.data.kernels.get(i));
        retain_selection(&mut self.users, data.users.len(), user.and_then(|old| {
            data.users.iter().position(|new| new.user == old.user)
        }));
        retain_selection(&mut self.kernels, data.kernels.len(), kernel.and_then(|old| {
            data.kernels.iter().position(|new| new.id == old.id)
        }));
        if !data.errors.iter().any(|error| error.starts_with("/kernels:")) {
            self.observe_kernel_idle(&data);
        }
        if !admin(data.identity.as_ref()) && self.is_admin() {
            self.pane = 0;
            self.detail = None;
            self.detail_error = None;
            self.modal = None;
            self.key_user = None;
        }
        self.data = data;
        self.loaded = true;
        if !self.tab.visible(self.is_admin()) {
            self.switch(Page::Overview);
        }
        if self.key_user.as_ref().is_some_and(|name| !self.data.users.iter().any(|user| &user.user == name)) {
            self.key_user = None;
            self.detail = None;
            self.modal = None;
        }
        if !self.history_user.as_ref().is_some_and(|name| self.history_users().contains(&name.as_str())) {
            self.history_user = self.history_users().first().map(|name| (*name).to_owned());
        }
    }

    fn observe_kernel_idle(&mut self, data: &Snapshot) {
        let now = Instant::now();
        let mut observations = HashMap::new();
        for kernel in data.kernels.iter().filter(|kernel| kernel.cpu_percent == Some(0.0)) {
            let old = self.idle_kernels.remove(&kernel.id).filter(|old|
                old.started == kernel.started && now.duration_since(old.last) <= Duration::from_secs(10));
            let first = old.map_or(now, |old| old.first);
            observations.insert(kernel.id.clone(), IdleObservation { started: kernel.started, first, last: now });
        }
        self.idle_kernels = observations;
    }

    fn history_users(&self) -> Vec<&str> {
        if self.is_admin() {
            let mut users: Vec<_> = self.data.users.iter().map(|user| user.user.as_str()).collect();
            if let Some(me) = &self.data.identity {
                if !users.contains(&me.user.as_str()) {
                    users.push(&me.user);
                }
            }
            users
        } else {
            self.data.identity.as_ref().filter(|me| me.role == "user").map(|me| me.user.as_str()).into_iter().collect()
        }
    }

    pub fn view(&self) -> View {
        match self.tab {
            Page::Overview => View::Overview,
            Page::Hardware => View::Hardware,
            Page::Wsl => View::Wsl,
            Page::Users => self.key_user.clone().map_or(View::None, View::Keys),
            Page::Policy => View::Policy,
            Page::History => View::History { metric: self.metric, user: METRICS[self.metric].starts_with("user_").then(|| self.history_user.clone()).flatten(), range: self.range },
            Page::Envs => View::Envs,
            Page::Audit => View::Audit,
            _ => View::None,
        }
    }

    pub fn update_detail(&mut self, view: View, result: Result<Detail, String>) {
        if view == self.view() {
            let error = match &result {
                Err(error) => Some(error.clone()),
                Ok(Detail::Overview(overview)) if !overview.errors.is_empty() => Some(overview.errors.join(" · ")),
                _ => None,
            };
            self.detail_error = error;
            if self.detail_error.is_some() {
                self.stale.get_or_insert_with(Instant::now);
                if self.detail.is_some() { return; }
            } else if self.data.errors.is_empty() {
                self.stale = None;
            }
            let len = match &result {
                Ok(Detail::Keys(items)) => items.len(),
                Ok(Detail::Envs(items)) => items.len(),
                Ok(Detail::Audit(items)) => items.len(),
                _ => 0,
            };
            let found = match (&self.detail, &result) {
                (Some(Ok(Detail::Keys(old))), Ok(Detail::Keys(new))) => self.rows.selected()
                    .and_then(|index| old.get(index))
                    .and_then(|key| new.iter().position(|entry| entry.fingerprint == key.fingerprint)),
                _ => None,
            };
            retain_selection(&mut self.rows, len, found);
            self.detail = Some(result);
        }
    }

    fn load(&mut self) -> Input {
        self.detail = None;
        self.detail_error = None;
        self.rows = TableState::default();
        Input::Command(Command::Load(self.view()))
    }

    fn switch(&mut self, tab: Page) -> Input {
        self.deferred = None;
        self.tab = tab;
        self.filter.clear();
        self.filtering = false;
        self.pane = 0;
        self.attention_row = 0;
        self.modal = None;
        self.key_user = None;
        self.load()
    }

    pub fn indices(&self, page: Page) -> Vec<usize> {
        let matches = |text: String| fuzzy(&self.filter, &text);
        match (page, &self.key_user, &self.detail) {
            (Page::Overview | Page::Users, None, _) => self.data.users.iter().enumerate()
                .filter(|(_, row)| matches(row.user.clone())).map(|(i, _)| i).collect(),
            (Page::Kernels, _, _) => self.data.kernels.iter().enumerate()
                .filter(|(_, row)| matches(format!("{} {} {} {}", row.id, row.user, row.pid, row.env.as_deref().unwrap_or(""))))
                .map(|(i, _)| i).collect(),
            (Page::Users, _, Some(Ok(Detail::Keys(rows)))) => rows.iter().enumerate()
                .filter(|(_, row)| matches(format!("{} {} {}", row.fingerprint, row.kind, row.comment))).map(|(i, _)| i).collect(),
            (Page::Envs, _, Some(Ok(Detail::Envs(rows)))) => rows.iter().enumerate()
                .filter(|(_, row)| matches(format!("{} {} {} {}", row.name, row.path, row.python.as_deref().unwrap_or(""), row.spyder_kernels.as_deref().unwrap_or(""))))
                .map(|(i, _)| i).collect(),
            (Page::Audit, _, Some(Ok(Detail::Audit(rows)))) => rows.iter().enumerate()
                .filter(|(_, row)| matches(format!("{} {} {} {} {} {}", row.actor, row.action, row.target, row.time, if row.ok { "success" } else { "failure" }, row.detail.as_deref().unwrap_or(""))))
                .map(|(i, _)| i).collect(),
            _ => Vec::new(),
        }
    }

    fn navigate(&mut self, code: KeyCode) {
        let page = self.focused_page();
        let indices = if page == Page::Overview {
            (0..self.attention().len()).collect()
        } else {
            self.indices(page)
        };
        let mut attention = TableState::default().with_selected(self.attention_row);
        let state = match page {
            Page::Overview => &mut attention,
            Page::Users if self.key_user.is_none() => &mut self.users,
            Page::Kernels => &mut self.kernels,
            _ => &mut self.rows,
        };
        if !indices.is_empty() {
            let index = state.selected().and_then(|row| indices.iter().position(|i| *i == row)).unwrap_or(0);
            let target = match code {
                KeyCode::Home | KeyCode::Char('g') => 0,
                KeyCode::End | KeyCode::Char('G') => indices.len() - 1,
                KeyCode::PageDown => (index + 10).min(indices.len() - 1),
                KeyCode::PageUp => index.saturating_sub(10),
                KeyCode::Down | KeyCode::Char('j') => (index + 1).min(indices.len() - 1),
                _ => index.saturating_sub(1),
            };
            state.select(Some(indices[target]));
        }
        self.attention_row = attention.selected().unwrap_or(0);
    }

    pub fn dashboard_home(&self) -> bool { self.layout == Layout::Dashboard && !self.dashboard_page }

    fn focused_page(&self) -> Page {
        if self.dashboard_home() {
            [Page::Overview, Page::Kernels, Page::Users][self.pane]
        } else {
            self.tab
        }
    }

    pub fn attention(&self) -> Vec<Attention> {
        let mut items = Vec::new();
        for user in &self.data.users {
            let Some(usage) = &user.usage else { continue; };
            let Some(mem) = usage.mem_current else { continue; };
            let reason = if usage.mem_high.is_some_and(|high| high > 0 && mem as f64 / high as f64 >= 0.9) {
                "near soft cap"
            } else if usage.mem_low.is_some_and(|low| mem > low) {
                "above guarantee"
            } else {
                continue;
            };
            let page = if self.is_admin() { Page::Users } else { Page::Kernels };
            items.push(Attention::warning(
                format!("{} {reason} ({:.1}G)", user.user, mem as f64 / (1u64 << 30) as f64),
                page,
            ));
        }
        for kernel in &self.data.kernels {
            let observed = self.idle_kernels.get(&kernel.id)
                .filter(|sample| sample.last.elapsed() <= Duration::from_secs(10))
                .map(|sample| sample.last.duration_since(sample.first).as_secs_f64());
            // The v1 API reports current CPU usage rather than an idle timestamp.
            if let Some((idle, rss)) = kernel.idle_s.or(observed).zip(kernel.mem_rss)
                .filter(|(idle, rss)| *idle > 1800.0 && *rss > 4 << 30)
            {
                items.push(Attention {
                    glyph: "i",
                    text: format!("{} kernel idle {:.0}m, {:.1}G held", kernel.user, idle / 60.0, rss as f64 / (1u64 << 30) as f64),
                    page: Page::Kernels,
                    time: None,
                });
            }
        }
        // 25% of VRAM is reserved for the local Windows operator (per-process cap is soft: cm_gpu_limit).
        for gpu in self.data.system.iter().flat_map(|system| system.gpus.iter().flatten()) {
            if let (Some(used), Some(total)) = (gpu.mem_used, gpu.mem_total) {
                if total > 0 && used as f64 / total as f64 > 0.75 {
                    items.push(Attention::warning(
                        format!("GPU{} VRAM {:.1}/{:.1}G > 75% · local reserve low", gpu.index,
                            used as f64 / (1u64 << 30) as f64, total as f64 / (1u64 << 30) as f64),
                        Page::Hardware,
                    ));
                }
            }
        }
        if self.is_admin() {
            if let Some(Ok(Detail::Overview(overview))) = &self.detail {
                if overview.restart_pending {
                    items.push(Attention::warning("WSL restart pending".into(), Page::Wsl));
                }
                for event in overview.audit.iter().filter(|entry| entry.action.contains("oomd") && entry.action.contains("kill")) {
                    items.push(Attention {
                        glyph: if event.ok { "✗" } else { "▲" },
                        text: if event.ok { format!("oomd killed {}", event.target) } else { format!("oomd kill failed for {}", event.target) },
                        page: Page::Audit,
                        time: Some(event.time),
                    });
                }
            }
        }
        if self.stale.is_some() {
            items.push(Attention::warning("Daemon degraded · r retry".into(), Page::Overview));
        }
        items.retain(|item| fuzzy(&self.filter, &item.text));
        items
    }

    fn set_layout(&mut self, layout: Layout) -> Input {
        self.layout = layout;
        if let Some(path) = &self.state_path {
            let _ = layout.save(path);
        }
        self.pane = 0;
        if layout == Layout::Dashboard {
            self.dashboard_page = false;
            return self.switch(Page::Overview);
        }
        Input::None
    }

    fn mutate(&mut self, action: Action) -> Input {
        self.modal = None;
        self.busy = true;
        self.busy_since = Some(Instant::now());
        self.status = Some(Ok("Applying change…".into()));
        Input::Command(Command::Mutate(action))
    }

    pub fn paste(&mut self, text: &str) {
        if self.too_small || self.restarted.is_some() {
            return;
        }
        match &mut self.modal {
            Some(Modal::Form(form)) => form.paste(text),
            Some(Modal::Palette { query, selected }) => {
                query.extend(text.chars().filter(|ch| !ch.is_control()).take(1024));
                *selected = 0;
            }
            None if self.filtering => {
                self.filter.extend(text.chars().filter(|ch| !ch.is_control()).take(1024));
                self.select_filter_match();
            }
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Input {
        if self.restarted.is_some() { return Input::None; }
        if self.too_small {
            return if key.code == KeyCode::Char('q') { Input::Quit } else { Input::None };
        }
        if let Some(Modal::Palette { query, selected }) = self.modal.take_if(|modal| matches!(modal, Modal::Palette { .. })) {
            return self.palette_key(key, query, selected);
        }
        if let Some(modal) = &mut self.modal {
            match modal {
                Modal::Form(form) => match key.code {
                    KeyCode::Esc => self.modal = None,
                    KeyCode::Enter if form.cancel_focused() => self.modal = None,
                    KeyCode::Enter => match form.submit() {
                        Ok(action) => return self.mutate(action),
                        Err(error) => form.error = Some(error),
                    },
                    _ => form.edit(key),
                },
                Modal::Confirm { action, .. } => match key.code {
                    KeyCode::Char('y') => {
                        let action = action.clone();
                        return self.mutate(action);
                    }
                    KeyCode::Char('n') | KeyCode::Esc | KeyCode::Enter => self.modal = None,
                    _ => {}
                },
                Modal::Palette { .. } => unreachable!(),
                Modal::Info { scroll, .. } => match key.code {
                    KeyCode::Enter if self.pane == 2 && self.tab == Page::Users => {
                        self.key_user = self.users.selected().and_then(|index| self.data.users.get(index)).map(|user| user.user.clone());
                        self.pane = 0;
                        self.modal = None;
                        self.filter.clear();
                        return self.load();
                    }
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char('?') => {
                        if self.pane == 2 { self.pane = 0; }
                        self.modal = None;
                    },
                    KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::Home => *scroll = 0,
                    _ => {}
                },
            }
            return Input::None;
        }

        if self.hardware_live {
            match key.code {
                KeyCode::Esc => self.hardware_live = false,
                KeyCode::Char('q') => return Input::Quit,
                KeyCode::Char('r') => return Input::Command(Command::Refresh(self.view())),
                _ => {}
            }
            return Input::None;
        }

        if self.filtering {
            match key.code {
                KeyCode::Esc => { self.filter.clear(); self.filtering = false; }
                KeyCode::Enter => self.filtering = false,
                KeyCode::Backspace => { self.filter.pop(); }
                KeyCode::Char(ch) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => self.filter.push(ch),
                _ => {}
            }
            self.select_filter_match();
            return Input::None;
        }
        match key.code {
            KeyCode::Char('q') => return Input::Quit,
            KeyCode::Char('h') if self.tab == Page::Overview => self.hardware_live = true,
            KeyCode::Char('l') if self.tab == Page::Hardware => self.hardware_live = true,
            KeyCode::Char('b') => return Input::Launch("btop"),
            KeyCode::Char('n') => return Input::Launch("nvitop"),
            KeyCode::Char('?') => self.modal = Some(Modal::Info { title: "Keys".into(), text: self.help(), scroll: 0 }),
            KeyCode::Char(':') => self.modal = Some(Modal::Palette { query: String::new(), selected: 0 }),
            KeyCode::Char('/') if self.tab.filterable() => self.filtering = true,
            KeyCode::Esc if !self.filter.is_empty() => self.filter.clear(),
            KeyCode::Esc if self.key_user.is_some() => return self.user_key(key.code),
            KeyCode::Esc if self.layout == Layout::Dashboard && self.dashboard_page => {
                self.dashboard_page = false;
                return self.switch(Page::Overview);
            }
            KeyCode::F(n @ 1..=3) => return self.set_layout(Layout::ALL[usize::from(n - 1)]),
            KeyCode::Char('v') => return self.set_layout(Layout::ALL[(self.layout as usize + 1) % 3]),
            KeyCode::Char(ch @ '1'..='9') => {
                let page = Page::ALL[(ch as usize) - ('1' as usize)];
                if page.visible(self.is_admin()) {
                    self.dashboard_page = self.layout == Layout::Dashboard;
                    return self.switch(page);
                }
            }
            KeyCode::Tab | KeyCode::BackTab if self.dashboard_home() => {
                let panes = if self.is_admin() { 3 } else { 2 };
                let step = if key.code == KeyCode::Tab { 1 } else { panes - 1 };
                self.pane = (self.pane + step) % panes;
            }
            KeyCode::Tab | KeyCode::BackTab if self.layout == Layout::Console && !self.narrow => {
                self.pane = 1 - self.pane;
            }
            KeyCode::Char('r') if self.tab != Page::History || self.stale.is_some() => return Input::Command(Command::Refresh(self.view())),
            code @ (KeyCode::Down | KeyCode::Up | KeyCode::Home | KeyCode::End | KeyCode::PageDown | KeyCode::PageUp | KeyCode::Char('j' | 'k' | 'g' | 'G')) => {
                if self.layout == Layout::Console && !self.narrow && self.pane == 1 {
                    let pages: Vec<_> = Page::ALL.into_iter().filter(|page| page.visible(self.is_admin())).collect();
                    let current = pages.iter().position(|page| *page == self.tab).unwrap_or(0);
                    let index = match code {
                        KeyCode::Home | KeyCode::PageUp | KeyCode::Char('g') => 0,
                        KeyCode::End | KeyCode::PageDown | KeyCode::Char('G') => pages.len() - 1,
                        KeyCode::Down | KeyCode::Char('j') => (current + 1).min(pages.len() - 1),
                        _ => current.saturating_sub(1),
                    };
                    if index != current {
                        let input = self.switch(pages[index]);
                        self.pane = 1;
                        return input;
                    }
                } else {
                    self.navigate(code);
                }
            }
            KeyCode::Enter if self.layout == Layout::Console && self.pane == 1 => self.pane = 0,
            _ => return self.tab_key(key.code),
        }
        Input::None
    }

    fn select_filter_match(&mut self) {
        let page = self.focused_page();
        let indices = self.indices(page);
        let state = match page {
            Page::Overview | Page::Users if self.key_user.is_none() => &mut self.users,
            Page::Kernels => &mut self.kernels,
            _ => &mut self.rows,
        };
        if !state.selected().is_some_and(|index| indices.contains(&index)) {
            state.select(indices.first().copied());
        }
        self.attention_row = 0;
    }

    fn palette_key(&mut self, key: KeyEvent, mut query: String, mut selected: usize) -> Input {
        let actions = self.actions(&query);
        match key.code {
            KeyCode::Esc => return Input::None,
            KeyCode::Enter => {
                return actions.get(selected).map_or(Input::None, |action| self.run_palette_action(action));
            }
            KeyCode::Down => selected = (selected + 1).min(actions.len().saturating_sub(1)),
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Backspace => { query.pop(); selected = 0; }
            KeyCode::Char(ch) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => { query.push(ch); selected = 0; }
            _ => {}
        }
        self.modal = Some(Modal::Palette { query, selected });
        Input::None
    }

    fn run_palette_action(&mut self, action: &PaletteAction) -> Input {
        if action.name == "Refresh now" {
            return Input::Command(Command::Refresh(self.view()));
        }
        let Some(page) = action.page else {
            return self.key(KeyEvent::new(action.code, KeyModifiers::NONE));
        };
        let mut load = Input::None;
        if self.tab != page || self.dashboard_home() || page == Page::Users && !action.keys && self.key_user.is_some() {
            self.dashboard_page = self.layout == Layout::Dashboard;
            load = self.switch(page);
        }
        if action.keys && self.key_user.is_none() {
            let Some(user) = self.users.selected().and_then(|i| self.data.users.get(i)) else { return load; };
            self.key_user = Some(user.user.clone());
            self.filter.clear();
            load = self.load();
        }
        if action.name == "Open SSH keys" {
            let Some(user) = self.users.selected().and_then(|i| self.data.users.get(i)) else { return load; };
            self.key_user = Some(user.user.clone());
            self.filter.clear();
            return self.load();
        }
        if self.detail.is_none() && (matches!(page, Page::Policy | Page::Wsl) && action.code == KeyCode::Char('e')
            || action.keys && action.code == KeyCode::Char('x'))
        {
            self.deferred = Some((self.view(), action.code));
            return Input::Command(Command::Load(self.view()));
        }
        let input = self.tab_key(action.code);
        if matches!(input, Input::None) { load } else { input }
    }

    pub fn finish_deferred(&mut self) -> Input {
        let Some((view, code)) = self.deferred.take() else { return Input::None; };
        if self.view() == view && self.modal.is_none() {
            self.tab_key(code)
        } else {
            Input::None
        }
    }

    fn tab_key(&mut self, code: KeyCode) -> Input {
        let page = self.focused_page();
        match page {
            Page::Overview if self.layout == Layout::Budget && self.is_admin() && !self.busy && code == KeyCode::Char('e') => {
                if let Some(Ok(Detail::Overview(overview))) = &self.detail {
                    if let Some(policy) = &overview.policy {
                        self.modal = Some(Modal::Form(self.policy_form(policy)));
                    }
                }
            }
            Page::Overview if code == KeyCode::Enter => {
                let items = self.attention();
                if let Some(item) = items.get(self.attention_row.min(items.len().saturating_sub(1))) {
                    if item.page == Page::Overview {
                        return Input::Command(Command::Refresh(self.view()));
                    }
                    self.dashboard_page = self.layout == Layout::Dashboard;
                    return self.switch(item.page);
                }
            }
            Page::Users | Page::Kernels if self.dashboard_home() && code == KeyCode::Enter => {
                self.dashboard_page = true;
                return self.switch(page);
            }
            Page::Users if self.dashboard_home() => {}
            Page::Kernels => {
                if let Some(kernel) = self.kernels.selected().filter(|i| self.indices(Page::Kernels).contains(i)).and_then(|index| self.data.kernels.get(index)) {
                    match code {
                        KeyCode::Enter => {
                            let text = format!("Owner  {}\nPID    {}\nKind   {}\nFile   {}",
                                kernel.user, kernel.pid, kernel.kind.as_deref().unwrap_or("n/a"),
                                kernel.connection_file.as_deref().unwrap_or("n/a"));
                            self.modal = Some(Modal::Info { title: kernel.id.clone(), text, scroll: 0 });
                        }
                        KeyCode::Char('s') if !self.busy && can_stop(self.data.identity.as_ref(), &kernel.user) => {
                            self.modal = Some(Modal::Confirm {
                                prompt: format!("Stop kernel {}?", kernel.id),
                                action: Action::Stop { id: kernel.id.clone(), owner: kernel.user.clone() },
                            });
                        }
                        KeyCode::Char('c') => {
                            let mut lines = Vec::new();
                            for host in &self.hosts {
                                let destination = format!("{}@{host}", kernel.user);
                                lines.push(format!("ssh -p 2200 {}", shell_word(&destination)));
                                if let Some(file) = kernel.connection_file.as_deref().filter(|file| !file.is_empty()) {
                                    let scp_host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.clone() };
                                    lines.push(format!("scp -P 2200 {} .", shell_word(&format!("{}@{scp_host}:{file}", kernel.user))));
                                }
                            }
                            lines.push(if kernel.connection_file.as_deref().is_none_or(str::is_empty) {
                                "Connection file is unavailable for this kernel.".into()
                            } else {
                                "In Spyder, choose Connect to an existing kernel, select the downloaded JSON, and enter the SSH host, port and user.".into()
                            });
                            let text = lines.join("\n");
                            self.modal = Some(Modal::Info { title: "Spyder connection".into(), text, scroll: 0 });
                        }
                        _ => {}
                    }
                }
            }
            Page::Users if self.is_admin() => return self.user_key(code),
            Page::Policy if self.is_admin() && !self.busy && code == KeyCode::Char('e') => {
                if let Some(Ok(Detail::Policy(policy))) = &self.detail {
                    self.modal = Some(Modal::Form(self.policy_form(policy)));
                }
            }
            Page::Wsl if self.is_admin() && !self.busy => match code {
                KeyCode::Char('e') => {
                    if let Some(Ok(Detail::Wsl(wsl, cores))) = &self.detail {
                        let cores = cores.or_else(|| self.data.system.as_ref()?.cpu.as_ref()?.cores);
                        if let Some(cores) = cores.filter(|cores| *cores >= 2) {
                            let config = wsl.config.clone().unwrap_or_default();
                            let fields = ["Memory (GiB)", "Processors", "Swap (GiB)"].into_iter()
                                .zip([config.memory_gb, config.processors, config.swap_gb])
                                .map(|(label, value)| (label, value.map(|n| n.to_string()).unwrap_or_default())).collect();
                            self.modal = Some(Modal::Form(Form::new(FormKind::Wsl(cores), fields)));
                        } else {
                            self.status = Some(Err("Host core count unavailable".into()));
                        }
                    }
                }
                KeyCode::Char('R') => self.modal = Some(Modal::Form(Form::new(FormKind::Restart, vec![("Confirm", String::new())]))),
                _ => {}
            },
            Page::History => {
                match code {
                    KeyCode::Char('m') => self.metric = cycle(self.metric, METRICS.len(), true),
                    KeyCode::Char('r' | 't') => self.range = cycle(self.range, RANGES.len(), true),
                    KeyCode::Char('u') if METRICS[self.metric].starts_with("user_") && self.is_admin() => {
                        let users = self.history_users();
                        if !users.is_empty() {
                            let current = users.iter().position(|user| Some(*user) == self.history_user.as_deref());
                            let next = current.map_or(0, |index| cycle(index, users.len(), true));
                            self.history_user = Some(users[next].into());
                        }
                    }
                    _ => return Input::None,
                }
                return self.load();
            }
            _ => {}
        }
        Input::None
    }

    fn policy_form(&self, policy: &CurrentPolicy) -> Form {
        let fields = POLICY_FIELDS.into_iter().zip(policy_values(policy)
            .map(|value| value.map(|n| n.to_string()).unwrap_or_default())).collect();
        let mut form = Form::new(FormKind::Policy, fields);
        form.user_count = self.data.users.len();
        form
    }

    fn user_key(&mut self, code: KeyCode) -> Input {
        if let Some(user) = &self.key_user {
            match code {
                KeyCode::Esc => { self.key_user = None; self.filter.clear(); return self.load(); }
                KeyCode::Char('a') if !self.busy => {
                    self.modal = Some(Modal::Form(Form::new(FormKind::AddKey(user.clone()), vec![("Public key", String::new())])));
                }
                KeyCode::Char('x') if !self.busy => {
                    if let Some(Ok(Detail::Keys(keys))) = &self.detail {
                        if let Some(key) = self.rows.selected().filter(|index| self.indices(Page::Users).contains(index)).and_then(|index| keys.get(index)) {
                            self.modal = Some(Modal::Confirm {
                                prompt: format!("Remove {} from {user}?", key.fingerprint),
                                action: Action::RemoveKey { user: user.clone(), fingerprint: key.fingerprint.clone() },
                            });
                        }
                    }
                }
                _ => {}
            }
        } else {
            let user = self.users.selected().filter(|index| self.indices(Page::Users).contains(index)).and_then(|index| self.data.users.get(index));
            match code {
                KeyCode::Enter => {
                    if let Some(user) = user {
                        if self.narrow {
                            self.modal = Some(Modal::Info { title: user.user.clone(), text: format!("{}\n\nEnter SSH keys · Esc close", self.user_detail(user)), scroll: 0 });
                            self.pane = 2;
                        } else {
                            self.key_user = Some(user.user.clone());
                            self.filter.clear();
                            return self.load();
                        }
                    }
                }
                KeyCode::Char('a') if !self.busy => {
                    self.modal = Some(Modal::Form(Form::new(FormKind::AddUser, vec![("Username", String::new()), ("Public key", String::new())])));
                }
                KeyCode::Char('A') if !self.busy => {
                    if let Some(user) = user {
                        let (role, prompt) = match user.role.as_deref() {
                            Some("admin") => (UserRole::User, format!("Revoke admin role from {}?", user.user)),
                            Some("user") => (UserRole::Admin, format!("Make {} an admin? They can manage users, policy and WSL", user.user)),
                            _ => return Input::None,
                        };
                        self.modal = Some(Modal::Confirm {
                            prompt,
                            action: Action::SetUserRole { user: user.user.clone(), role },
                        });
                    }
                }
                KeyCode::Char('d') if !self.busy => {
                    if let Some(user) = user {
                        self.modal = Some(Modal::Form(Form::new(FormKind::Disable(user.user.clone()), vec![("Username", String::new())])));
                    }
                }
                _ => {}
            }
        }
        Input::None
    }

    pub fn user_detail(&self, user: &crate::api::User) -> String {
        let memory = |value: Option<u64>| value.map_or("n/a".into(), |n| format!("{:.1}G", n as f64 / (1u64 << 30) as f64));
        let count = |value: Option<u64>| value.map_or("n/a".into(), |n| n.to_string());
        let usage = user.usage.as_ref();
        let envs = self.data.kernels.iter().filter(|kernel| kernel.user == user.user)
            .filter_map(|kernel| kernel.env.as_deref()).collect::<Vec<_>>().join(", ");
        format!("Memory   {} · guarantee {} · soft cap {}\nKernels  {}  {}\nSSH keys {}\nLinger   {} · sessions {} · UID {} · role {}",
            memory(usage.and_then(|u| u.mem_current)), memory(usage.and_then(|u| u.mem_low)),
            memory(usage.and_then(|u| u.mem_high)), count(user.kernels), envs, count(user.keys),
            user.linger.map_or("n/a", |on| if on { "on" } else { "off" }), count(user.sessions),
            count(user.uid.map(u64::from)), user.role.as_deref().unwrap_or("n/a"))
    }

    pub fn shortcuts(&self) -> &'static str {
        if self.hardware_live {
            return "esc back  r refresh";
        }
        if self.dashboard_home() {
            return if self.pane == 1 {
                "h live  tab pane  s stop  c connect  enter open  1-9 page"
            } else {
                "h live  tab pane  ↑↓ select  enter open  1-9 page"
            };
        }
        match self.tab {
            Page::Overview if self.layout == Layout::Budget && self.is_admin() => "h live  e edit policy  enter open  1-9 page  F1-3 layout",
            Page::Overview => "h live  ↑↓ select  enter open  1-9 page  F1-3 layout",
            Page::Kernels => "↑↓ select  s stop  c connect  enter detail  / filter",
            Page::Users if !self.is_admin() => "",
            Page::Users if self.key_user.is_some() => "↑↓ select  a add key  x remove key  esc users",
            Page::Users if self.narrow => "↑↓ select  a add  d disable  A role  enter detail  / filter",
            Page::Users => "↑↓ select  a add  d disable  A role  enter keys  / filter",
            Page::Policy => "e edit policy  r refresh",
            Page::Wsl => "e edit  R restart WSL  r refresh  F1-3 layout",
            Page::History if self.stale.is_some() => "m metric  t range  r retry",
            Page::History if self.is_admin() && METRICS[self.metric].starts_with("user_") => "m metric  u user  r range",
            Page::History => "m metric  r range",
            Page::Hardware => "l live  r refresh  1-9 page  F1-3 layout",
            Page::Envs | Page::Audit => "↑↓ scroll  / filter  r refresh",
        }
    }

    pub fn actions(&self, query: &str) -> Vec<PaletteAction> {
        use KeyCode::{Char, F};
        let mut actions = vec![
            PaletteAction::new("Console layout", "F1", F(1)), PaletteAction::new("Dashboard layout", "F2", F(2)),
            PaletteAction::new("Budget layout", "F3", F(3)), PaletteAction::new("Cycle layout", "v", Char('v')),
            PaletteAction::new("Next pane", "tab", KeyCode::Tab), PaletteAction::new("Previous pane", "shift-tab", KeyCode::BackTab),
            PaletteAction::new("Move down", "↓ j", KeyCode::Down), PaletteAction::new("Move up", "↑ k", KeyCode::Up),
            PaletteAction::new("Next list page", "PgDn", KeyCode::PageDown), PaletteAction::new("Previous list page", "PgUp", KeyCode::PageUp),
            PaletteAction::new("Top of list", "g", Char('g')), PaletteAction::new("Bottom of list", "G", Char('G')),
            PaletteAction::new("Open selection", "enter", KeyCode::Enter), PaletteAction::new("Back", "esc", KeyCode::Esc),
            PaletteAction::new("Filter table", "/", Char('/')), PaletteAction::new("Command palette", ":", Char(':')),
            PaletteAction::new("Refresh now", "r", Char('r')), PaletteAction::new("Launch btop", "b", Char('b')),
            PaletteAction::new("Launch nvitop", "n", Char('n')), PaletteAction::new("Help", "?", Char('?')),
            PaletteAction::new("Quit", "q", Char('q')),
        ];
        if let Some((hint, key)) = match self.tab {
            Page::Overview => Some(("h", 'h')),
            Page::Hardware => Some(("l", 'l')),
            _ => None,
        } {
            actions.push(PaletteAction::new("Hardware live", hint, Char(key)));
        }
        for page in Page::ALL.into_iter().filter(|page| page.visible(self.is_admin())) {
            let label = page.label();
            actions.push(PaletteAction::new(&label[2..], &label[..1], Char(char::from(label.as_bytes()[0]))));
        }
        for (page, name, key, code) in [
            (Page::Kernels, "Stop kernel", "s", Char('s')), (Page::Kernels, "Spyder connect", "c", Char('c')),
            (Page::Users, "Add user", "a", Char('a')), (Page::Users, "Disable user", "d", Char('d')),
            (Page::Users, "Toggle admin role", "A", Char('A')),
            (Page::Users, "Open SSH keys", "enter", KeyCode::Enter),
            (Page::Policy, "Edit policy", "e", Char('e')), (Page::Wsl, "Edit WSL configuration", "e", Char('e')),
            (Page::Wsl, "Restart WSL", "R", Char('R')), (Page::History, "History metric", "m", Char('m')),
            (Page::History, "History user", "u", Char('u')), (Page::History, "History range", "r", Char('r')),
        ] {
            if page.visible(self.is_admin()) && (name != "History user" || self.is_admin()) {
                actions.push(PaletteAction { page: Some(page), ..PaletteAction::new(name, key, code) });
            }
        }
        if self.is_admin() {
            actions.extend([
                PaletteAction { page: Some(Page::Users), keys: true, ..PaletteAction::new("Add SSH key", "a", Char('a')) },
                PaletteAction { page: Some(Page::Users), keys: true, ..PaletteAction::new("Remove SSH key", "x", Char('x')) },
            ]);
        }
        actions.retain(|action| fuzzy(query, &action.name));
        actions.sort_by_key(|action| !action.name.to_ascii_lowercase().starts_with(&query.to_ascii_lowercase()));
        actions
    }

    fn help(&self) -> String {
        format!("F1 F2 F3    Console / Dashboard / Budget\nv           Cycle layout\n1-9         Page ({})\ntab         Next pane · shift-tab previous pane\n↑↓ j k      Move · PgUp PgDn page · g G top/bottom\nenter       Open / drill in · esc back\n/           Filter table · esc clears\n:           Command palette\nr           Refresh/retry · history range\nb n         btop / nvitop\nh / l       Hardware live from Overview / Hardware\n? q         Help / quit\n\n{}", Page::ALL.into_iter().filter(|page| page.visible(self.is_admin())).map(Page::label).collect::<Vec<_>>().join(" · "), self.actions("").into_iter().filter(|action| action.page.is_some() || action.name.ends_with(" SSH key")).map(|action| format!("{:<11} {}", action.key, action.name)).collect::<Vec<_>>().join("\n"))
    }
}

pub struct Attention {
    pub glyph: &'static str,
    pub text: String,
    pub page: Page,
    pub time: Option<f64>,
}

impl Attention {
    fn warning(text: String, page: Page) -> Self { Self { glyph: "▲", text, page, time: None } }
}

pub struct PaletteAction {
    pub name: String,
    pub key: String,
    pub code: KeyCode,
    pub page: Option<Page>,
    keys: bool,
}

impl PaletteAction {
    fn new(name: &str, key: &str, code: KeyCode) -> Self {
        Self { name: name.into(), key: key.into(), code, page: None, keys: false }
    }
}

fn cycle(index: usize, len: usize, forward: bool) -> usize {
    if forward { (index + 1) % len } else { (index + len - 1) % len }
}

fn retain_selection(state: &mut TableState, len: usize, found: Option<usize>) {
    state.select((len > 0).then(|| found.unwrap_or_else(|| state.selected().unwrap_or(0).min(len - 1))));
}

fn shell_word(value: &str) -> String {
    if value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte)) {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Identity;

    #[test]
    fn validates_forms_and_accepts_pasted_public_keys() {
        for good in ["a", "_user-42", &"a".repeat(31)] { assert!(valid_username(good)); }
        for bad in ["", "A", "1a", "a.b", "a\n", "aé", &"a".repeat(32)] { assert!(!valid_username(bad)); }
        assert!(parse_policy(["4", "4", "8", "100"]).is_ok());
        for bad in [["9", "4", "8", "100"], ["4", "9", "8", "100"], ["4", "4", "8", "1.2"], ["-1", "4", "8", "100"]] {
            assert!(parse_policy(bad).is_err());
        }
        let mut form = Form::new(FormKind::AddUser, vec![("Username", "alice".into()), ("Public key", String::new())]);
        form.focus(true);
        form.paste("ssh-ed25519 MOCK demo comment\r\n");
        assert_eq!(form.submit().unwrap(), Action::AddUser { user: "alice".into(), pubkey: "ssh-ed25519 MOCK demo comment".into() });
        let disable = Form::new(FormKind::Disable("alice".into()), vec![("Username", "Alice".into())]);
        assert!(disable.submit().is_err());
    }

    #[test]
    fn validates_wsl_boundaries_and_typed_restart() {
        for values in [["16", "2", "0"], ["124", "24", "64"]] {
            assert!(parse_wsl(values, 24).is_ok());
        }
        for values in [
            ["15", "2", "0"], ["125", "2", "0"], ["16", "1", "0"],
            ["16", "25", "0"], ["16", "2", "-1"], ["16", "2", "65"],
            ["16.5", "2", "0"],
        ] {
            assert!(parse_wsl(values, 24).is_err(), "{values:?}");
        }
        let mut form = Form::new(FormKind::Restart, vec![("Confirm", String::new())]);
        assert!(form.submit().is_err());
        form.paste("restart");
        assert_eq!(form.submit(), Ok(Action::RestartWsl));
    }

    #[test]
    fn dashboard_page_returns_home_and_hidden_keys_do_not_switch() {
        let mut app = Dashboard { layout: Layout::Dashboard, ..Dashboard::default() };
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        app.key(key(KeyCode::Char('6')));
        assert!(app.dashboard_home());
        app.key(key(KeyCode::Char('1')));
        assert!(!app.dashboard_home());
        app.key(key(KeyCode::Esc));
        assert!(app.dashboard_home());
    }

    #[test]
    fn gates_number_keys_by_identity() {
        let mut app = Dashboard::default();
        app.data.identity = Some(Identity { user: "alice".into(), role: "user".into() });
        for number in ['2', '4', '6', '9'] {
            app.key(KeyEvent::new(KeyCode::Char(number), KeyModifiers::NONE));
            assert!(app.tab == Page::Overview);
        }
        for (number, expected) in [('3', Page::Kernels), ('5', Page::Hardware), ('7', Page::Envs), ('8', Page::History)] {
            app.key(KeyEvent::new(KeyCode::Char(number), KeyModifiers::NONE));
            assert!(app.tab == expected);
        }
        app.data.identity.as_mut().unwrap().role = "admin".into();
        app.key(KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE));
        assert!(app.tab == Page::Audit);
    }
}
