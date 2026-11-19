use crate::api::{History, System, LIVE_WINDOW};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_SAMPLES: usize = 301;
const MAX_CORES: usize = 24;

type Series = VecDeque<(f64, Option<f64>)>;

#[derive(Default)]
pub struct HardwareHistory {
    metrics: BTreeMap<&'static str, Series>,
    cores: Vec<Series>,
    seeded: BTreeSet<&'static str>,
    live_start: Option<f64>,
    last_stream: Option<f64>,
    end: Option<f64>,
}

impl HardwareHistory {
    pub fn series(&self, metric: &str) -> Option<&Series> {
        self.metrics.get(metric)
    }

    pub fn core_series(&self, index: usize) -> Option<&Series> {
        self.cores.get(index)
    }

    fn seed(&mut self, histories: &BTreeMap<&'static str, History>) {
        for (&metric, history) in histories {
            if !self.seeded.insert(metric) {
                continue;
            }
            let series = self.metrics.entry(metric).or_default();
            for &(time, value) in &history.points {
                if self.live_start.is_none_or(|start| time < start) {
                    insert(series, time, value);
                }
            }
        }
    }

    pub fn update(
        &mut self,
        histories: &BTreeMap<&'static str, History>,
        streamed: &VecDeque<System>,
        latest: Option<&System>,
    ) {
        self.seed(histories);
        for system in streamed {
            if let Some(time) = system.time.filter(|time| time.is_finite()) {
                if self.last_stream.is_none_or(|last| time > last) {
                    self.sample(system, time);
                    self.last_stream = Some(time);
                }
            }
        }
        if let Some((system, time)) = latest
            .and_then(|system| system.time.map(|time| (system, time)))
            .filter(|(_, time)| time.is_finite() && self.end.is_none_or(|end| *time > end))
        {
            self.sample(system, time);
        }
        if let Some(end) = self.end {
            for series in self.metrics.values_mut().chain(&mut self.cores) {
                while series.front().is_some_and(|(time, _)| *time < end - LIVE_WINDOW as f64)
                    || series.len() > MAX_SAMPLES
                {
                    series.pop_front();
                }
            }
        }
    }

    fn sample(&mut self, system: &System, time: f64) {
        self.end = Some(self.end.map_or(time, |end| end.max(time)));
        self.live_start = Some(self.live_start.map_or(time, |start| start.min(time)));
        let cpu = system.cpu.as_ref();
        let gpus = system.gpus.as_deref().unwrap_or_default();
        let net = system.net.as_deref().unwrap_or_default();
        let disk = system.disk_io.as_deref().unwrap_or_default();
        for (name, value) in [
            ("cpu_percent", cpu.and_then(|cpu| cpu.percent)),
            ("mem_used", system.mem.as_ref().and_then(|mem| mem.used).map(|bytes| bytes as f64)),
            ("gpu_mem_used", total(gpus.iter().filter_map(|gpu| gpu.mem_used.map(|bytes| bytes as f64)))),
            ("gpu_util", average(gpus.iter().filter_map(|gpu| gpu.util_percent))),
            ("gpu_temp", average(gpus.iter().filter_map(|gpu| gpu.temp_c))),
            ("net_rx", total(net.iter().filter_map(|iface| iface.rx_bps))),
            ("net_tx", total(net.iter().filter_map(|iface| iface.tx_bps))),
            ("disk_read", total(disk.iter().filter_map(|dev| dev.read_bps))),
            ("disk_write", total(disk.iter().filter_map(|dev| dev.write_bps))),
        ] {
            insert(self.metrics.entry(name).or_default(), time, value);
        }
        let values = cpu.and_then(|cpu| cpu.per_core.as_deref()).unwrap_or_default();
        self.cores.resize_with(self.cores.len().max(values.len().min(MAX_CORES)), VecDeque::new);
        for (index, series) in self.cores.iter_mut().enumerate() {
            insert(series, time, values.get(index).copied().flatten());
        }
    }
}

fn insert(series: &mut Series, time: f64, value: Option<f64>) {
    if !time.is_finite() {
        return;
    }
    let value = value.filter(|value| value.is_finite() && *value >= 0.0);
    if series.back().is_none_or(|(last, _)| time > *last) {
        series.push_back((time, value));
        return;
    }
    match series.binary_search_by(|(existing, _)| existing.total_cmp(&time)) {
        Ok(index) => series[index].1 = value,
        Err(index) => series.insert(index, (time, value)),
    }
}

fn total(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.reduce(|sum, value| sum + value)
}

fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0), |(sum, count), value| (sum + value, count + 1));
    (count > 0).then(|| sum / f64::from(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(time: f64, percent: f64) -> System {
        serde_json::from_value(serde_json::json!({
            "hostname": "test", "time": time,
            "cpu": {"percent": percent, "per_core": [12.0, null]},
        })).unwrap()
    }

    #[test]
    fn seeds_once_merges_stream_and_expires_old_points() {
        let mut ring = HardwareHistory::default();
        let seed = BTreeMap::from([("cpu_percent", History {
            points: vec![(40.0, Some(1.0)), (100.0, None), (300.0, Some(3.0))],
            window: None,
        })]);
        let first = sample(300.0, 60.0);
        ring.update(&seed, &VecDeque::new(), Some(&first));
        let streamed = VecDeque::from([sample(300.0, 61.0), sample(302.0, 62.0), sample(304.0, 63.0)]);
        ring.update(&seed, &streamed, streamed.back());
        ring.update(&seed, &streamed, streamed.back());
        assert_eq!(ring.series("cpu_percent").unwrap(), &VecDeque::from([
            (40.0, Some(1.0)), (100.0, None), (300.0, Some(61.0)), (302.0, Some(62.0)), (304.0, Some(63.0)),
        ]));
        assert_eq!(ring.core_series(1).unwrap().back(), Some(&(304.0, None)));
        ring.update(&seed, &streamed, Some(&sample(402.0, 40.0)));
        assert_eq!(ring.series("cpu_percent").unwrap().front(), Some(&(300.0, Some(61.0))));
    }

    #[test]
    fn accepts_late_seed_and_bounds_dense_samples() {
        let mut ring = HardwareHistory::default();
        ring.update(&BTreeMap::new(), &VecDeque::new(), Some(&sample(300.0, 20.0)));
        ring.update(&BTreeMap::from([("cpu_percent", History {
            points: vec![(290.0, None), (300.0, Some(99.0))],
            window: None,
        })]), &VecDeque::new(), None);
        assert_eq!(ring.series("cpu_percent").unwrap(), &VecDeque::from([
            (290.0, None), (300.0, Some(20.0)),
        ]));
        let streamed = (1..=350).map(|n| sample(300.0 + f64::from(n) / 2.0, 50.0)).collect();
        ring.update(&BTreeMap::new(), &streamed, None);
        assert_eq!(ring.series("cpu_percent").unwrap().len(), MAX_SAMPLES);
        assert_eq!(ring.core_series(0).unwrap().len(), MAX_SAMPLES);
    }
}
