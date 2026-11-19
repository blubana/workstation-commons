mod api;
mod app;
mod config;
mod live_history;
mod live_ui;
mod theme;
mod ui;

use api::{Detail, MockState, Snapshot, Source, StreamMetrics, View};
use app::{Command, Dashboard, Input, Page};
use config::Layout;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::{CrosstermBackend, TestBackend}, Terminal};
use std::{
    env,
    io::{self, Stdout},
    path::PathBuf,
    process::{Command as Process, ExitCode},
    sync::{
        mpsc,
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};


const REFRESH: Duration = Duration::from_secs(2);
const USAGE: &str = "Usage: cm-top [--mock <dir>] [--frame <layout>:<page> --cols C --rows R]";

type DetailUpdate = (View, Result<Detail, String>);
type Update = (Snapshot, Option<DetailUpdate>);
type Notice = (bool, Result<String, String>);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cm-top: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut mock = None;
    let mut frame = None;
    let mut cols = None;
    let mut rows = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "-h" | "--help" => { println!("{USAGE}"); return Ok(()); }
            "--mock" => mock = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--frame" => {
                let value = args.next().ok_or(USAGE)?;
                let (layout, page) = value.split_once(':').ok_or(USAGE)?;
                frame = Some((Layout::parse(layout).ok_or(USAGE)?, Page::parse(page).ok_or(USAGE)?));
            }
            "--cols" => cols = Some(dimension(args.next())?),
            "--rows" => rows = Some(dimension(args.next())?),
            _ => return Err(USAGE.into()),
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    if let Some((layout, page)) = frame {
        let source = Source::Mock(mock.unwrap_or_else(|| "fixtures".into()));
        let mut app = runtime.block_on(fixture_app(&source, layout, page));
        let mut terminal = Terminal::new(TestBackend::new(cols.unwrap_or(120), rows.unwrap_or(30)))?;
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let buffer = terminal.backend().buffer();
        for row in buffer.content.chunks(usize::from(buffer.area.width)) {
            println!("{}", row.iter().map(|cell| cell.symbol()).collect::<String>());
        }
        return Ok(());
    }
    if cols.is_some() || rows.is_some() { return Err(USAGE.into()); }
    let source = mock.map_or_else(|| Source::Socket(
        env::var_os("CM_SOCK").map(PathBuf::from)
            .unwrap_or_else(|| "/run/compute-mgr/api.sock".into()),
    ), Source::Mock);
    let mut screen = Screen::enter()?;
    let (commands, incoming) = tokio::sync::mpsc::unbounded_channel::<Command>();
    let (outgoing, notices) = mpsc::channel::<Notice>();
    let latest = Arc::new(Mutex::new(None::<Update>));
    let publisher = Arc::clone(&latest);
    thread::Builder::new().name("cm-top-api".into()).spawn(move || {
        runtime.block_on(api_worker(source, incoming, outgoing, publisher));
    })?;

    let state_path = config::state_path();
    let mut app = Dashboard {
        layout: state_path.as_deref().map(Layout::load).unwrap_or_default(),
        state_path,
        hosts: config::hosts(),
        ..Dashboard::default()
    };
    loop {
        let update = latest.lock().expect("snapshot lock poisoned").take();
        if let Some((data, detail)) = update {
            let previous_view = app.view();
            app.update(data);
            if app.view() != previous_view {
                commands.send(Command::Load(app.view()))?;
            }
            if let Some((view, result)) = detail {
                let deferred = app.deferred.as_ref().is_some_and(|(target, _)| *target == view);
                app.update_detail(view, result);
                if deferred {
                    if let Input::Command(command) = app.finish_deferred() {
                        commands.send(command)?;
                    }
                }
            }
        }
        match notices.try_recv() {
            Ok((restart, status)) => {
                app.busy = false;
                if restart && status.is_ok() {
                    app.restarted = Some(Instant::now());
                }
                app.status = Some(status);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Err("API worker exited".into()),
        }
        screen.terminal.draw(|frame| ui::draw(frame, &mut app))?;
        if app.restarted.is_some_and(|time| time.elapsed() >= Duration::from_secs(3)) {
            break;
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Paste(text) => app.paste(&text),
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                match app.key(key) {
                    Input::None => {}
                    Input::Quit => break,
                    Input::Command(command) => commands.send(command)?,
                    Input::Launch(tool) => app.status = screen.launch(tool)?.map(Err),
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn dimension(value: Option<String>) -> Result<u16, Box<dyn std::error::Error>> {
    value.and_then(|value| value.parse().ok()).filter(|size| (1..=1000).contains(size))
        .ok_or_else(|| "Frame dimensions must be between 1 and 1000".into())
}

async fn fixture_app(source: &Source, layout: Layout, page: Page) -> Dashboard {
    let http = api::client();
    let mut mock = MockState::default();
    let mut app = Dashboard { layout, ..Dashboard::default() };
    app.update(source.current(&http, &mut mock, &StreamMetrics::default()).await);
    if page.visible(app.is_admin()) {
        app.tab = page;
        app.dashboard_page = layout == Layout::Dashboard && page != Page::Overview;
    }
    let view = app.view();
    let detail = source.detail(&http, &mut mock, &view, app.data.identity.as_ref()).await;
    app.update_detail(view, detail);
    app
}

async fn api_worker(
    source: Source,
    mut incoming: tokio::sync::mpsc::UnboundedReceiver<Command>,
    outgoing: mpsc::Sender<Notice>,
    publisher: Arc<Mutex<Option<Update>>>,
) {
    let http = api::client();
    let stream = source.start_stream(&http);
    let mut mock = MockState::default();
    let mut view = View::Overview;
    let mut mutation_at = None;
    let empty = StreamMetrics::default();
    let mut snapshot = source.current(&http, &mut mock, &empty).await;
    let mut next = Instant::now() + REFRESH;
    *publisher.lock().expect("snapshot lock poisoned") = Some((snapshot.clone(), None));
    let detail = source.detail(&http, &mut mock, &view, snapshot.identity.as_ref()).await;
    *publisher.lock().expect("snapshot lock poisoned") = Some((snapshot.clone(), Some((view.clone(), detail))));
    loop {
        let mut refresh = false;
        let mut action = None;
        let mut load = false;
        let mut force = false;
        let deadline = mock.next_npu.map_or(next, |npu| next.min(npu));
        match tokio::time::timeout_at(deadline.into(), incoming.recv()).await {
            Ok(Some(Command::Load(target))) => { view = target; load = true; }
            Ok(Some(Command::Refresh(target))) => { view = target; force = true; }
            Ok(Some(Command::Mutate(pending))) => action = Some(pending),
            Ok(None) => break,
            Err(_) => refresh = true,
        }
        // Keep the newest filter when keys are pressed faster than a request completes.
        while action.is_none() {
            match incoming.try_recv() {
                Ok(Command::Load(target)) => { view = target; load = true; }
                Ok(Command::Refresh(target)) => { view = target; force = true; }
                Ok(Command::Mutate(pending)) => action = Some(pending),
                Err(_) => break,
            }
        }
        let notice = if let Some(action) = action {
            let restart = matches!(action, api::Action::RestartWsl);
            let result = source.mutate(&http, &mut mock, action).await;
            if restart && result.is_ok() {
                if outgoing.send((true, result)).is_err() { break; }
                // The daemon shuts down shortly after accepting the restart.
                std::future::pending::<()>().await;
                return;
            }
            // Read through to the API immediately following a mutation.
            mutation_at = Some(Instant::now());
            snapshot = source.current(&http, &mut mock, &empty).await;
            next = Instant::now() + REFRESH;
            Some((restart, result))
        } else {
            if refresh || force {
                let streamed = stream.as_ref().map(|receiver| receiver.borrow().clone()).unwrap_or_default();
                let streamed = if force { empty.clone() } else { streamed.after(mutation_at) };
                snapshot = source.current(&http, &mut mock, &streamed).await;
                next = Instant::now() + REFRESH;
            }
            None
        };
        let periodic = refresh && matches!(view, View::Audit | View::History { .. } | View::Overview | View::Wsl);
        let detail = if view != View::None && (load || force || notice.is_some() || periodic) {
            let result = source.detail(&http, &mut mock, &view, snapshot.identity.as_ref()).await;
            Some((view.clone(), result))
        } else {
            None
        };
        let mut latest = publisher.lock().expect("snapshot lock poisoned");
        let pending = latest.take().and_then(|(_, detail)| detail).filter(|(target, _)| target == &view);
        *latest = Some((snapshot.clone(), detail.or(pending)));
        drop(latest);
        if let Some(notice) = notice {
            if outgoing.send(notice).is_err() {
                break;
            }
        }
    }
}

struct Screen {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Screen {
    fn enter() -> io::Result<Self> {
        let mut screen = Self { terminal: Terminal::new(CrosstermBackend::new(io::stdout()))? };
        screen.resume()?;
        Ok(screen)
    }

    fn resume(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(self.terminal.backend_mut(), EnterAlternateScreen, EnableBracketedPaste)?;
        self.terminal.hide_cursor()?;
        self.terminal.clear()
    }

    fn suspend(&mut self) -> io::Result<()> {
        let cursor = self.terminal.show_cursor();
        let leave = execute!(self.terminal.backend_mut(), DisableBracketedPaste, LeaveAlternateScreen);
        let raw = disable_raw_mode();
        cursor.and(leave).and(raw)
    }

    fn launch(&mut self, tool: &str) -> io::Result<Option<String>> {
        self.suspend()?;
        let outcome = Process::new(tool).status();
        self.resume()?;
        Ok(match outcome {
            Ok(status) if status.success() => None,
            Ok(status) => Some(format!("{tool} exited with {status}")),
            Err(error) => Some(format!("Could not launch {tool}: {error}")),
        })
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}
