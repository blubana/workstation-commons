use crate::{
    api::{Gpu, NpuActive, NpuStatus, LIVE_WINDOW},
    app::Dashboard,
    theme::{style, Token},
    ui::{block, clean, percent, ratio, size},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use std::collections::VecDeque;

type Series = VecDeque<(f64, Option<f64>)>;

const WINDOW: f64 = LIVE_WINDOW as f64;
const CORE_LIMIT: usize = 24;
const CORE_WIDTH: usize = 18;
const SPARK_WIDTH: usize = 5;
pub(crate) const NPU_DETAIL_HEIGHT: u16 = 6;

pub(crate) fn strip(frame: &mut Frame, app: &Dashboard, area: Rect) -> Rect {
    let screen = frame.area();
    let npu_height = if let Some(status @ NpuStatus { up: Some(true), .. }) = &app.data.npu {
        2 + npu_summary(status, area.width.saturating_sub(2)).len() as u16
    } else {
        3
    };
    let wide = screen.width >= 160 && screen.height >= 45;
    let compact_view = compact_preview(screen, area);
    let height = if compact_view {
        6
    } else {
        let cpu_width = cpu_width(area.width);
        let columns = core_columns(cpu_width.saturating_sub(2), wide);
        let core_rows = CORE_LIMIT.div_ceil(columns) as u16;
        let requested = if wide { 29 } else { core_rows + 10 };
        requested.min(area.height.saturating_sub(npu_height + 9))
    };
    let [hardware, npu, remaining] = Layout::vertical([
        Constraint::Length(height), Constraint::Length(npu_height), Constraint::Fill(1),
    ]).areas(area);
    if compact_view {
        compact(frame, app, hardware);
    } else {
        boxes(frame, app, hardware, wide, false);
    }
    npu_panel(frame, app, npu, false);
    remaining
}

pub(crate) fn fullscreen(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let outer = block(" Hardware live ", true);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let [charts, npu] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(NPU_DETAIL_HEIGHT)]).areas(inner);
    let screen = frame.area();
    boxes(
        frame,
        app,
        charts,
        screen.width >= 160 && screen.height >= 45,
        true,
    );
    npu_panel(frame, app, npu, true);
}

pub(crate) fn npu_panel(frame: &mut Frame, app: &Dashboard, area: Rect, details: bool) {
    let inner = open_box(frame, area, " NPU ");
    let status = app.data.npu.as_ref();
    let mut lines = match status {
        Some(NpuStatus { up: Some(false), .. }) => vec![Line::styled("✗ NPU worker down", style(Token::Error))],
        Some(status @ NpuStatus { up: Some(true), .. }) => npu_summary(status, inner.width),
        _ => vec![Line::styled("NPU status n/a", style(Token::Faint))],
    };
    if details {
        let (devices, version) = status.map_or((None, None), |status| (status.devices.as_ref(), status.openvino.as_deref()));
        let devices = devices.map(|devices| devices.iter().map(|name| clean(name)).collect::<Vec<_>>().join(", "));
        lines.push(Line::raw(format!("Devices {}", devices.as_deref().filter(|names| !names.is_empty()).unwrap_or("n/a"))));
        let mut runtime = format!("OpenVINO {}", clean(version.unwrap_or("n/a")));
        if let Some(models) = status.and_then(|status| status.cached_models) {
            runtime.push_str(&format!(" · cached models {models}"));
        }
        if let Some(errors) = status.and_then(|status| status.errors) {
            runtime.push_str(&format!(" · errors {errors}"));
        }
        lines.push(Line::raw(runtime));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn npu_summary(status: &NpuStatus, width: u16) -> Vec<Line<'static>> {
    let count = |value: Option<u64>| value.map_or_else(|| "n/a".into(), |n| n.to_string());
    let device = status.npu.as_ref();
    let name = device.and_then(|device| device.name.as_deref()).unwrap_or("n/a");
    let driver = device.and_then(|device| device.npu_driver_version.as_ref())
        .map_or_else(|| "n/a".into(), |driver| driver.as_str().map(clean).unwrap_or_else(|| driver.to_string()));
    let active = status.active.as_ref().and_then(NpuActive::user).unwrap_or("n/a");
    let queued = status.queued.as_ref().and_then(|users| {
        users.values().try_fold(0u64, |total, n| Some(total.saturating_add((*n)?)))
    });
    let average = status.avg_latency_s.map_or_else(|| "n/a".into(), |n| format!("{n:.2}s"));
    let mut summary = Line::from(vec![
        Span::raw(format!("NPU {} ", clean(name))),
        Span::styled("✓ up", style(Token::Success)),
        Span::raw(format!(" · driver {driver} · active {}", clean(active))),
    ]);
    let stats = format!("queued {} · served {} · avg {average}", count(queued), count(status.served));
    if summary.width() + 3 + stats.len() <= usize::from(width) {
        summary.spans.push(Span::raw(format!(" · {stats}")));
        vec![summary]
    } else {
        vec![summary, Line::raw(stats)]
    }
}

fn compact_preview(screen: Rect, content: Rect) -> bool {
    let columns = core_columns(cpu_width(content.width).saturating_sub(2), false);
    let core_rows = CORE_LIMIT.div_ceil(columns) as u16;
    screen.width < 100 || content.height < core_rows + 18
}

fn cpu_width(width: u16) -> u16 {
    (width - width / 3).max(74).min(width.saturating_sub(22))
}

fn boxes(frame: &mut Frame, app: &Dashboard, area: Rect, wide: bool, full: bool) {
    let cpu_width = cpu_width(area.width);
    let columns = core_columns(cpu_width.saturating_sub(2), wide);
    let grid_rows = CORE_LIMIT.div_ceil(columns) as u16;
    let bottom_height = if full || wide {
        (area.height / 2).saturating_sub(1).max(6)
    } else {
        6
    };
    let top_height = (area.height.saturating_sub(bottom_height)).max(grid_rows + 3);
    let [top, bottom] =
        Layout::vertical([Constraint::Length(top_height), Constraint::Fill(1)]).areas(area);
    let [cpu, mem] =
        Layout::horizontal([Constraint::Length(cpu_width), Constraint::Fill(1)]).areas(top);
    let [gpu, net, disk] = Layout::horizontal([Constraint::Fill(1); 3]).areas(bottom);
    cpu_panel(frame, app, cpu, wide);
    mem_panel(frame, app, mem);
    gpu_panel(frame, app, gpu);
    net_panel(frame, app, net);
    disk_panel(frame, app, disk);
}

fn open_box(frame: &mut Frame, area: Rect, title: impl Into<Line<'static>>) -> Rect {
    let border = block(title, false);
    let inner = border.inner(area);
    frame.render_widget(border, area);
    inner
}

fn compact(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let system = app.data.system.as_ref();
    let cpu = system.and_then(|s| s.cpu.as_ref());
    let mem = system.and_then(|s| s.mem.as_ref());
    let gpus = system.and_then(|s| s.gpus.as_deref()).unwrap_or_default();
    let [cpu_area, gauges] =
        Layout::vertical([Constraint::Length(5), Constraint::Length(1)]).areas(area);
    let [chart, core] =
        Layout::vertical([Constraint::Length(2), Constraint::Length(1)]).areas(open_box(
            frame,
            cpu_area,
            " CPU ",
        ));
    let value = cpu.and_then(|c| c.percent);
    graph(
        frame,
        chart,
        app.hardware_history.series("cpu_percent"),
        format!("Total {}", percent(value)),
        Unit::Percent,
    );
    let cores = cpu.and_then(|c| c.cores);
    let label = cores.map_or_else(|| "Cores n/a".into(), |n| format!("{n} cores"));
    meter(frame, core, &label, value, &percent(value));
    let [mem_area, gpu_area] = Layout::horizontal([Constraint::Fill(1); 2]).areas(gauges);
    let mem_value = ratio(mem.and_then(|m| m.used), mem.and_then(|m| m.total));
    let gpu_value = gpu_total(gpus, |g| g.util_percent, true);
    meter(frame, mem_area, "MEM", mem_value, &percent(mem_value));
    meter(frame, gpu_area, "GPU", gpu_value, &percent(gpu_value));
}

fn core_columns(width: u16, spark: bool) -> usize {
    let cell = CORE_WIDTH + if spark { SPARK_WIDTH } else { 0 };
    (usize::from(width) / cell).clamp(2, 4)
}

fn cpu_panel(frame: &mut Frame, app: &Dashboard, area: Rect, spark: bool) {
    let cpu = app.data.system.as_ref().and_then(|s| s.cpu.as_ref());
    let cores = cpu.and_then(|c| c.per_core.as_deref()).unwrap_or_default();
    let reported = cpu.and_then(|c| c.cores).unwrap_or_default();
    let count = cores.len().max(reported.min(CORE_LIMIT as u64) as usize);
    let count = count.min(CORE_LIMIT);
    let inner = open_box(frame, area, " CPU ");
    let cols = core_columns(inner.width, spark);
    let rows = count.div_ceil(cols).min(usize::from(inner.height.saturating_sub(1)));
    let [chart, grid] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(rows as u16)]).areas(inner);
    let freq = reading(cpu.and_then(|c| c.freq_mhz), "MHz");
    let caption = format!("Total {}  {freq}", percent(cpu.and_then(|c| c.percent)));
    graph(
        frame,
        chart,
        app.hardware_history.series("cpu_percent"),
        caption,
        Unit::Percent,
    );

    let column_width = usize::from(grid.width) / cols;
    for row in 0..rows {
        let mut spans = Vec::new();
        for col in 0..cols {
            let i = row * cols + col;
            if i >= count {
                break;
            }
            let value = cores.get(i).copied().flatten();
            spans.push(Span::raw(format!("C{i:02} ")));
            spans.extend(bar(value, 6));
            spans.push(Span::raw(format!(" {:>4}", percent(value))));
            let spark_width = column_width.saturating_sub(CORE_WIDTH);
            if spark && spark_width >= 2 {
                let history = app.hardware_history.core_series(i);
                spans.push(Span::raw(" "));
                spans.extend(sparkline(history, spark_width - 1));
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::raw(" ".repeat(spark_width + 1)));
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(grid.x, grid.y + row as u16, grid.width, 1),
        );
    }
}

fn mem_panel(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let system = app.data.system.as_ref();
    let mem = system.and_then(|s| s.mem.as_ref());
    let breakdown = mem.and_then(|m| m.breakdown.as_ref());
    let total = mem.and_then(|m| m.total);
    let used = breakdown.and_then(|m| m.used).or_else(|| mem.and_then(|m| m.used));
    let cached = breakdown.and_then(|m| m.cached);
    let available = breakdown.and_then(|m| m.available);
    let buffers = breakdown.and_then(|m| m.buffers);
    let inner = open_box(frame, area, format!(" MEM {} ", size(total)));
    let buffer_rows = u16::from(inner.height >= 10);
    let swap_rows = u16::from(inner.height >= 12);
    let [used_area, cached_area, available_area, buffer_area, swap_area, chart] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(buffer_rows),
        Constraint::Length(swap_rows),
        Constraint::Fill(1),
    ]).areas(inner);
    for (area, label, value) in [
        (used_area, "Used", used),
        (cached_area, "Cached", cached),
        (available_area, "Available", available),
    ] {
        meter(frame, area, label, ratio(value, total), &size(value));
    }
    if buffer_rows > 0 {
        meter(frame, buffer_area, "Buffers", ratio(buffers, total), &size(buffers));
    }
    if swap_rows > 0 {
        let swap = system.and_then(|s| s.swap.as_ref());
        let value = swap.and_then(|s| s.used);
        meter(frame, swap_area, "Swap", ratio(value, swap.and_then(|s| s.total)), &size(value));
    }
    graph(
        frame,
        chart,
        app.hardware_history.series("mem_used"),
        "RAM used".into(),
        Unit::Bytes,
    );
}

fn gpu_total(gpus: &[Gpu], metric: impl Fn(&Gpu) -> Option<f64>, average: bool) -> Option<f64> {
    let (sum, count) = gpus.iter().filter_map(metric).fold((0.0, 0), |(sum, count), n| {
        (sum + n, count + 1)
    });
    (count > 0).then(|| if average { sum / f64::from(count) } else { sum })
}

fn gpu_panel(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let gpus = app
        .data
        .system
        .as_ref()
        .and_then(|s| s.gpus.as_deref())
        .unwrap_or_default();
    let Some(primary) = gpus.first() else {
        let inner = open_box(frame, area, " GPU ");
        frame.render_widget(Paragraph::new("GPU metrics n/a").style(style(Token::Faint)), inner);
        return;
    };
    let title = primary.name.as_deref().map_or_else(
        || format!(" GPU {} ", primary.index),
        |name| format!(" GPU {} {} ", primary.index, clean(name)),
    );
    let inner = open_box(frame, area, title);
    let util = gpu_total(gpus, |g| g.util_percent, true);
    let vram = gpu_total(gpus, |g| g.mem_used.map(|n| n as f64), false);
    let details = match inner.height {
        10.. => 4,
        8..=9 => 3,
        _ => 2,
    };
    let temp_rows = if inner.height >= 11 { 2 } else { 0 };
    let [util_area, vram_area, readings, temp] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Length(details),
        Constraint::Length(temp_rows),
    ]).areas(inner);
    graph(
        frame,
        util_area,
        app.hardware_history.series("gpu_util"),
        format!("Util {}", percent(util)),
        Unit::Percent,
    );
    graph(
        frame,
        vram_area,
        app.hardware_history.series("gpu_mem_used"),
        format!("VRAM {}", Unit::Bytes.format(vram)),
        Unit::Bytes,
    );
    let temp_reading = reading(primary.temp_c, "C");
    let power = reading(primary.power_w, "W");
    let clock = reading(primary.clock_mhz, "MHz");
    let fan = percent(primary.fan_percent);
    let mut lines = vec![
        Line::raw(format!("Temp {temp_reading}  Power {power}")),
        Line::raw(format!("Clock {clock}  Fan {fan}")),
    ];
    if details > 2 {
        let enc = percent(primary.enc_util);
        let dec = percent(primary.dec_util);
        lines.push(Line::raw(format!("Encoder {enc}  Decoder {dec}")));
    }
    if details > 3 {
        let vram_total = gpu_total(gpus, |g| g.mem_total.map(|n| n as f64), false);
        lines.push(Line::raw(format!("VRAM capacity {}", Unit::Bytes.format(vram_total))));
    }
    frame.render_widget(Paragraph::new(lines), readings);
    if temp_rows > 0 {
        graph(
            frame,
            temp,
            app.hardware_history.series("gpu_temp"),
            "GPU temperature".into(),
            Unit::Temperature,
        );
    }
}

fn net_panel(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let devices = app.data.system.as_ref().and_then(|s| s.net.as_deref()).unwrap_or_default();
    let inner = open_box(frame, area, " NET ");
    let interface_rows = if inner.height >= 9 { inner.height - 6 } else { 0 };
    let [rx, tx, interfaces] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Length(interface_rows),
    ]).areas(inner);
    for (area, metric, label, value) in [
        (rx, "net_rx", "RX", sum(devices.iter().filter_map(|n| n.rx_bps))),
        (tx, "net_tx", "TX", sum(devices.iter().filter_map(|n| n.tx_bps))),
    ] {
        graph(
            frame,
            area,
            app.hardware_history.series(metric),
            format!("{label} {}", Unit::Rate.format(value)),
            Unit::Rate,
        );
    }
    if interface_rows > 0 {
        let lines = devices.iter().take(usize::from(interface_rows)).map(|net| {
            Line::raw(format!(
                "{} RX {} TX {}",
                clean(&net.iface),
                Unit::Rate.format(net.rx_bps),
                Unit::Rate.format(net.tx_bps),
            ))
        }).collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), interfaces);
    }
}

fn disk_panel(frame: &mut Frame, app: &Dashboard, area: Rect) {
    let devices = app
        .data
        .system
        .as_ref()
        .and_then(|s| s.disk_io.as_deref())
        .unwrap_or_default();
    let inner = open_box(frame, area, " DISK ");
    let device_rows = if inner.height >= 10 { inner.height - 8 } else { 0 };
    let [read, write, root, windows, device_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(device_rows),
    ]).areas(inner);
    for (area, metric, label, value) in [
        (read, "disk_read", "Read", sum(devices.iter().filter_map(|d| d.read_bps))),
        (write, "disk_write", "Write", sum(devices.iter().filter_map(|d| d.write_bps))),
    ] {
        graph(
            frame,
            area,
            app.hardware_history.series(metric),
            format!("{label} {}", Unit::Rate.format(value)),
            Unit::Rate,
        );
    }
    let volumes = app.data.disks.iter().flat_map(|disks| {
        disks.linux.iter().flatten().chain(disks.windows.iter().flatten())
    });
    for (area, mount) in [(root, "/"), (windows, "C:")] {
        let volume = volumes.clone().find(|volume| match volume.mount.as_deref() {
            Some("/mnt/c" | "C:\\" | "C:/") => mount == "C:",
            Some(path) => path == mount,
            None => false,
        });
        let usage = ratio(volume.and_then(|v| v.used), volume.and_then(|v| v.total));
        meter(frame, area, mount, usage, &percent(usage));
    }
    if device_rows > 0 {
        let lines = devices.iter().take(usize::from(device_rows)).map(|disk| {
            Line::raw(format!(
                "{} R {} W {}",
                clean(&disk.dev),
                Unit::Rate.format(disk.read_bps),
                Unit::Rate.format(disk.write_bps),
            ))
        }).collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), device_area);
    }
}

fn sum(mut values: impl Iterator<Item = f64>) -> Option<f64> {
    values.next().map(|first| first + values.sum::<f64>())
}

fn reading(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(|| "n/a".into(), |n| format!("{n:.0}{unit}"))
}

fn ramp(level: f64) -> Token {
    if level < 1.0 / 3.0 {
        Token::Success
    } else if level < 2.0 / 3.0 {
        Token::Warn
    } else {
        Token::Error
    }
}

fn bar(value: Option<f64>, width: usize) -> Vec<Span<'static>> {
    let level = value.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    let fill = (width as f64 * level).round() as usize;
    vec![
        Span::styled("▕", style(Token::Faint)),
        Span::styled("█".repeat(fill), style(ramp(level))),
        Span::styled("░".repeat(width - fill), style(Token::Faint)),
        Span::styled("▏", style(Token::Faint)),
    ]
}

fn meter(frame: &mut Frame, area: Rect, label: &str, value: Option<f64>, text: &str) {
    let width = usize::from(area.width).saturating_sub(label.len() + text.len() + 4);
    let mut spans = vec![Span::raw(format!("{label} "))];
    spans.extend(bar(value, width));
    spans.push(Span::styled(
        format!(" {text}"),
        if value.is_some() { Default::default() } else { style(Token::Faint) },
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn sparkline(history: Option<&Series>, width: usize) -> Vec<Span<'static>> {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let values = history.into_iter().flat_map(|points| points.iter().map(|(_, n)| *n));
    let length = history.map_or(0, VecDeque::len);
    let step = length.div_ceil(width).max(1);
    let ticks = length.div_ceil(step);
    let mut spans = vec![Span::raw(" ".repeat(width.saturating_sub(ticks)))];
    for group in 0..ticks {
        let max = values.clone().skip(group * step).take(step).flatten().reduce(f64::max);
        spans.push(max.map_or_else(
            || Span::raw(" "),
            |value| {
                let level = value.clamp(0.0, 100.0) / 100.0;
                Span::styled(LEVELS[(level * 7.0).round() as usize].to_string(), style(ramp(level)))
            },
        ));
    }
    spans
}

#[derive(Clone, Copy)]
enum Unit {
    Percent,
    Bytes,
    Rate,
    Temperature,
}

impl Unit {
    fn format(self, value: Option<f64>) -> String {
        let Some(value) = value else { return "n/a".into() };
        match self {
            Self::Percent => percent(Some(value)),
            Self::Temperature => format!("{value:.0}C"),
            Self::Bytes | Self::Rate => {
                let amount = if value < 1_024.0 {
                    format!("{value:.0}B")
                } else {
                    size(Some(value as u64))
                };
                if matches!(self, Self::Rate) { format!("{amount}/s") } else { amount }
            }
        }
    }
}

fn graph(
    frame: &mut Frame,
    area: Rect,
    series: Option<&Series>,
    caption: String,
    unit: Unit,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let maximum = series.and_then(scale);
    let value = unit.format(maximum);
    let legend = format!("{caption}  max {value}");
    let plot_area = if area.height == 1 {
        let available = usize::from(area.width.saturating_sub(4));
        let legend = if legend.chars().count() > available {
            let max_caption = available.saturating_sub(value.len() + 2);
            let short_caption: String = caption.chars().take(max_caption).collect();
            format!("{} ↑{value}", short_caption.trim_end())
        } else {
            legend
        };
        let legend_width = legend.chars().count() as u16;
        let [caption_area, plot] =
            Layout::horizontal([Constraint::Length(legend_width + 1), Constraint::Fill(1)])
                .areas(area);
        frame.render_widget(Paragraph::new(legend), caption_area);
        plot
    } else {
        let [caption_area, plot] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        frame.render_widget(Paragraph::new(legend), caption_area);
        plot
    };
    if let (Some(series), Some(maximum)) = (series, maximum) {
        frame.render_widget(
            Paragraph::new(plot(series, maximum, plot_area.width, plot_area.height)),
            plot_area,
        );
    }
}

fn scale(series: &Series) -> Option<f64> {
    let end = series.back()?.0;
    series.iter().filter_map(|(t, n)| valid(end, *t, *n)).reduce(f64::max)
}

fn valid(end: f64, time: f64, value: Option<f64>) -> Option<f64> {
    value.filter(|n| time >= end - WINDOW && n.is_finite() && *n >= 0.0)
}

const DOTS: [[u8; 4]; 2] = [[1, 2, 4, 64], [8, 16, 32, 128]];

fn plot(series: &Series, maximum: f64, width: u16, height: u16) -> Vec<Line<'static>> {
    let (columns, rows) = (usize::from(width), usize::from(height));
    let Some((end, _)) = series.back().filter(|_| columns > 0 && rows > 0) else {
        return Vec::new();
    };
    let last_x = (columns * 2 - 1) as f64;
    let last_y = (rows * 4 - 1) as f64;
    let mut cells = vec![(0u8, 0.0f64); columns * rows];
    let mut mark = |x: f64, y: f64| {
        let x = x.round() as usize;
        let y = y.round() as usize;
        let cell = &mut cells[y / 4 * columns + x / 2];
        cell.0 |= DOTS[x % 2][y % 4];
        cell.1 = cell.1.max(1.0 - y as f64 / last_y);
    };
    let mut previous: Option<(f64, f64, f64)> = None;
    for &(t, n) in series {
        let Some(value) = valid(*end, t, n) else {
            previous = None;
            continue;
        };
        let x = ((t - (end - WINDOW)) / WINDOW).clamp(0.0, 1.0) * last_x;
        let y = last_y * (1.0 - if maximum > 0.0 { value / maximum } else { 0.0 });
        if let Some((_, from_x, from_y)) = previous.filter(|(old_t, _, _)| t - old_t <= 25.0) {
            let steps = (x - from_x).abs().max((y - from_y).abs()).ceil() as usize;
            for step in 0..steps {
                let f = step as f64 / steps as f64;
                mark(from_x + f * (x - from_x), from_y + f * (y - from_y));
            }
        }
        mark(x, y);
        previous = Some((t, x, y));
    }
    cells.chunks(columns).map(|row| {
        Line::from(row.iter().map(|&(dots, level)| {
            if dots == 0 {
                Span::raw(" ")
            } else {
                let glyph = char::from_u32(0x2800 + u32::from(dots)).unwrap();
                Span::styled(glyph.to_string(), style(ramp(level)))
            }
        }).collect::<Vec<_>>())
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_scales_to_window_max_and_ignores_missing_values() {
        let points = VecDeque::from([(0.0, Some(900.0)), (1.0, Some(800.0)), (302.0, Some(0.0)), (312.0, None), (322.0, Some(63.0))]);
        assert_eq!(scale(&points), Some(63.0));
        let rendered = plot(&points, 63.0, 20, 3);
        assert_eq!(rendered[0].spans[19].content, "⠈");
        assert!(rendered[1].spans.iter().all(|s| s.content == " "));
        let zeros = VecDeque::from([(0.0, Some(0.0))]);
        assert_eq!(scale(&zeros), Some(0.0));
        assert_eq!(plot(&zeros, 0.0, 1, 1)[0].spans[0].content, "⢀");
        assert_eq!(scale(&VecDeque::from([(0.0, None)])), None);
    }

    #[test]
    fn core_grid_uses_available_width_and_reserves_space_for_wide_history() {
        let narrow = Rect::new(0, 0, 80, 24);
        assert!(compact_preview(narrow, Rect::new(0, 0, 80, 20)));
        for (screen, body_width, body_height, spark) in [
            (Rect::new(0, 0, 120, 30), 104, 27, false),
            (Rect::new(0, 0, 160, 45), 144, 42, true),
        ] {
            assert!(!compact_preview(screen, Rect::new(0, 0, body_width, body_height)));
            let width = cpu_width(body_width) - 2;
            let columns = core_columns(width, spark);
            assert_eq!(columns, 4);
            assert!(usize::from(width) / columns >= CORE_WIDTH + usize::from(spark) * SPARK_WIDTH);
        }
        let full_narrow_width = cpu_width(78) - 2;
        assert_eq!(core_columns(full_narrow_width, false), 3);
        assert_eq!(core_columns(36, false), 2);
    }
}
