use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Bytes, Method, Request};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixConnector, Uri};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type HttpClient = Client<UnixConnector, Full<Bytes>>;
type Result<T> = std::result::Result<T, String>;

#[derive(Clone)]
pub enum Source {
    Socket(PathBuf),
    Mock(PathBuf),
}

#[derive(Clone, Deserialize)]
pub struct Identity {
    pub user: String,
    pub role: String,
}

#[derive(Clone, Deserialize)]
pub struct System {
    pub hostname: String,
    pub uptime_s: Option<f64>,
    pub time: Option<f64>,
    pub cpu: Option<Cpu>,
    pub mem: Option<Capacity>,
    pub swap: Option<Capacity>,
    pub gpus: Option<Vec<Gpu>>,
    pub oomd: Option<Oomd>,
    pub net: Option<Vec<NetIo>>,
    pub disk_io: Option<Vec<DiskIo>>,
}

#[derive(Clone, Deserialize)]
pub struct NetIo {
    pub iface: String,
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
}

#[derive(Clone, Deserialize)]
pub struct DiskIo {
    pub dev: String,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
}

#[derive(Clone, Deserialize)]
pub struct Cpu {
    pub cores: Option<u64>,
    pub percent: Option<f64>,
    pub per_core: Option<Vec<Option<f64>>>,
    pub freq_mhz: Option<f64>,
}

#[derive(Clone, Deserialize)]
pub struct Capacity {
    pub user_slice_max: Option<u64>,
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub breakdown: Option<MemoryBreakdown>,
}

#[derive(Clone, Deserialize)]
pub struct MemoryBreakdown {
    pub used: Option<u64>,
    pub cached: Option<u64>,
    pub buffers: Option<u64>,
    pub available: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct Gpu {
    pub index: u32,
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
    pub name: Option<String>,
    pub util_percent: Option<f64>,
    pub mem_total: Option<u64>,
    pub mem_used: Option<u64>,
    pub enc_util: Option<f64>,
    pub dec_util: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub fan_percent: Option<f64>,
}

#[derive(Clone, Default, Deserialize)]
pub struct NpuStatus {
    pub up: Option<bool>,
    pub npu: Option<NpuDevice>,
    pub devices: Option<Vec<String>>,
    pub openvino: Option<String>,
    pub cached_models: Option<u64>,
    pub active: Option<NpuActive>,
    pub queued: Option<BTreeMap<String, Option<u64>>>,
    pub served: Option<u64>,
    pub errors: Option<u64>,
    pub avg_latency_s: Option<f64>,
}

#[derive(Clone, Deserialize)]
pub struct NpuDevice {
    pub name: Option<String>,
    pub npu_driver_version: Option<serde_json::Value>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum NpuActive {
    User(String),
    Job { user: Option<String> },
}

impl NpuActive {
    pub fn user(&self) -> Option<&str> {
        match self {
            Self::User(user) => Some(user),
            Self::Job { user } => user.as_deref(),
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct Oomd {
    pub active: Option<bool>,
}

#[derive(Clone, Deserialize)]
pub struct User {
    pub user: String,
    pub usage: Option<Usage>,
    pub uid: Option<u32>,
    pub role: Option<String>,
    pub linger: Option<bool>,
    pub keys: Option<u64>,
    pub sessions: Option<u64>,
    pub kernels: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct Usage {
    pub mem_current: Option<u64>,
    pub mem_low: Option<u64>,
    pub mem_high: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub gpu_active: Option<bool>,
}

#[derive(Clone, Deserialize)]
pub struct Kernel {
    pub id: String,
    pub user: String,
    pub pid: u32,
    pub started: Option<f64>,
    pub idle_s: Option<f64>,
    pub connection_file: Option<String>,
    pub env: Option<String>,
    pub kind: Option<String>,
    pub cpu_percent: Option<f64>,
    pub mem_rss: Option<u64>,
    pub gpu_active: Option<bool>,
}

#[derive(Clone, Deserialize)]
pub struct History {
    pub points: Vec<(f64, Option<f64>)>,
    #[serde(skip)]
    pub window: Option<[f64; 2]>,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub identity: Option<Identity>,
    pub system: Option<System>,
    pub users: Vec<User>,
    pub kernels: Vec<Kernel>,
    pub hardware_history: Arc<BTreeMap<&'static str, History>>,
    pub disks: Option<Disks>,
    pub npu: Option<NpuStatus>,
    pub stream_samples: Arc<VecDeque<System>>,
    pub errors: Vec<String>,
}

pub fn can_stop(identity: Option<&Identity>, owner: &str) -> bool {
    identity.is_some_and(|me| me.role == "admin" || (me.role == "user" && me.user == owner))
}

impl Source {
    async fn get<T: DeserializeOwned>(&self, http: &HttpClient, route: &str, fixture: &str) -> Result<T> {
        let body = match self {
            Self::Mock(dir) => fs::read(dir.join(fixture)).map_err(|e| e.to_string())?,
            Self::Socket(_) => self.request(http, Method::GET, route, None).await?.to_vec(),
        };
        serde_json::from_slice(&body).map_err(|e| e.to_string())
    }

    async fn get_or_stream<T: Clone + DeserializeOwned>(
        &self, http: &HttpClient, route: &str, fixture: &str, sample: &Option<(std::time::Instant, T)>,
    ) -> Result<T> {
        if let Some((_, data)) = sample.as_ref().filter(|(time, _)| time.elapsed() < Duration::from_secs(5)) {
            return Ok(data.clone());
        }
        self.get(http, route, fixture).await
    }

    async fn snapshot(&self, http: &HttpClient, stream: &StreamMetrics) -> Snapshot {
        let (identity, system, users, kernels) = tokio::join!(
            self.get(http, "/auth/me", "auth-me.json"),
            self.get_or_stream(http, "/system", "system.json", &stream.system),
            self.get_or_stream(http, "/users", "users.json", &stream.users),
            self.get_or_stream(http, "/kernels", "kernels.json", &stream.kernels),
        );
        let mut errors = Vec::new();
        let mut data = Snapshot {
            identity: record(identity, "/auth/me", &mut errors),
            system: record(system, "/system", &mut errors),
            users: record(users, "/users", &mut errors).unwrap_or_default(),
            kernels: record(kernels, "/kernels", &mut errors).unwrap_or_default(),
            stream_samples: Arc::clone(&stream.samples),
            errors,
            ..Snapshot::default()
        };
        scope(&mut data);
        data
    }

    pub fn start_stream(&self, http: &HttpClient) -> Option<tokio::sync::watch::Receiver<StreamMetrics>> {
        let Self::Socket(path) = self else { return None; };
        let uri = Uri::new(path, "/api/v1/stream").into();
        let http = http.clone();
        let (sender, receiver) = tokio::sync::watch::channel(StreamMetrics::default());
        tokio::spawn(async move {
            loop {
                let _ = stream_once(&http, &uri, &sender).await;
                sender.send_modify(|metrics| {
                    let samples = Arc::clone(&metrics.samples);
                    *metrics = StreamMetrics { samples, ..StreamMetrics::default() };
                });
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
        Some(receiver)
    }

    async fn request(&self, http: &HttpClient, method: Method, route: &str, json: Option<serde_json::Value>) -> Result<Bytes> {
        let Self::Socket(path) = self else { return Err("socket unavailable in mock mode".into()); };
        let uri: hyper::Uri = Uri::new(path, &format!("/api/v1{route}")).into();
        let mut builder = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header("host", "localhost");
        if json.is_some() { builder = builder.header("content-type", "application/json"); }
        let request = builder.body(Full::new(json.map_or_else(Bytes::new, |value| Bytes::from(value.to_string()))))
            .map_err(|e| e.to_string())?;
        tokio::time::timeout(Duration::from_secs(if method == Method::GET { 2 } else { 15 }), async {
            let response = http.request(request).await.map_err(|e| {
                format!("{}: {e}", path.display())
            })?;
            let status = response.status();
            let body = Limited::new(response.into_body(), 8 * 1024 * 1024)
                .collect().await.map_err(|e| e.to_string())?.to_bytes();
            if status.is_success() {
                return Ok(body);
            }
            let detail = serde_json::from_slice::<ApiError>(&body).ok()
                .map(|e| format!(" ({}) {}", e.error, e.detail)).unwrap_or_default();
            Err(format!("HTTP {status}{detail}"))
        }).await.map_err(|_| "request timed out".to_owned())?
    }
}

pub fn client() -> HttpClient {
    Client::builder(TokioExecutor::new()).build(UnixConnector)
}

#[derive(Clone, Deserialize)]
struct ApiError {
    error: String,
    detail: String,
}

fn record<T>(result: Result<T>, route: &str, errors: &mut Vec<String>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(format!("{route}: {error}"));
            None
        }
    }
}

fn encode_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_wsl_scalar_shapes() {
        let wsl: Wsl = serde_json::from_str(r#"{
            "distros":[{"name":"Ubuntu-24.04","version":"2"}],
            "config":{"vm_idle_timeout":"-1"}, "restart_pending":true
        }"#).unwrap();
        assert_eq!(wsl.distros.unwrap()[0].version, Some(2));
        assert_eq!(wsl.config.unwrap().vm_idle_timeout, Some(-1));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_wsl_edit_and_restart_are_kept_in_memory() {
        let source = Source::Mock(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"));
        let http = client();
        let mut state = MockState::default();
        let initial = source.current(&http, &mut state, &StreamMetrics::default()).await;
        let identity = initial.identity.as_ref();
        let edit = WslEdit { memory_gb: 64, processors: 8, swap_gb: 0 };
        let (method, route, body) = Action::SetWsl(edit.clone()).request();
        assert_eq!((method, route, body), (Method::PUT, "/wsl/config".into(),
            Some(serde_json::json!({"memory_gb":64,"processors":8,"swap_gb":0}))));
        source.mutate(&http, &mut state, Action::SetWsl(edit)).await.unwrap();
        let Detail::Wsl(wsl, _) = source.detail(&http, &mut state, &View::Wsl, identity).await.unwrap() else { panic!("expected WSL detail"); };
        assert_eq!(wsl.restart_pending, Some(true));
        let config = wsl.config.unwrap();
        assert_eq!((config.memory_gb, config.processors, config.swap_gb), (Some(64), Some(8), Some(0)));
        assert_eq!(Action::RestartWsl.request(), (Method::POST, "/wsl/restart".into(), None));
        source.mutate(&http, &mut state, Action::RestartWsl).await.unwrap();
        let restarted = source.current(&http, &mut state, &StreamMetrics::default()).await;
        assert!(restarted.kernels.is_empty());
        assert_eq!(source.wsl(&http, &state).await.unwrap().restart_pending, Some(false));
    }

    #[test]
    fn parses_null_metrics() {
        let system: System = serde_json::from_str(include_str!("../fixtures/system.json")).unwrap();
        let gpu = &system.gpus.as_ref().unwrap()[1];
        assert_eq!(gpu.mem_used, None);
        assert_eq!(gpu.util_percent, None);
        assert_eq!(system.swap.unwrap().used, None);

        let users: Vec<User> = serde_json::from_str(include_str!("../fixtures/users.json")).unwrap();
        assert_eq!(users[1].usage.as_ref().unwrap().gpu_active, None);
        let kernels: Vec<Kernel> = serde_json::from_str(include_str!("../fixtures/kernels.json")).unwrap();
        assert_eq!(kernels[1].mem_rss, None);
        let history: History =
            serde_json::from_str(include_str!("../fixtures/history-cpu_percent.json")).unwrap();
        assert_eq!(history.points[1].1, None);
    }

    #[test]
    fn parses_unconfigured_policy_and_chunked_sse() {
        let policy: CurrentPolicy = serde_json::from_str(
            r#"{"mem_low_gb":null,"mem_high_gb":80,"total_max_gb":null,"cpu_weight":100}"#,
        ).unwrap();
        assert_eq!(policy.mem_low_gb, None);
        assert_eq!(policy.mem_high_gb, Some(80));

        let mut parser = SseParser::default();
        let mut metrics = StreamMetrics::default();
        for chunk in [b"event: ker".as_slice(), b"nels\r\ndata: [\r\n", b"data: ]\r\n\r\n: ping\n\n"] {
            parser.push(chunk, |name, json| assert!(metrics.event(name, json))).unwrap();
        }
        assert_eq!(metrics.kernels.unwrap().1.len(), 0);
    }

    #[test]
    fn stream_keeps_every_distinct_system_sample_inside_live_window() {
        let mut stream = StreamMetrics::default();
        for timestamp in [42, 43, 44, 44, 342, 343] {
            let json = format!(r#"{{"hostname":"fixture","time":{timestamp}}}"#);
            assert!(stream.event("system", &json));
        }
        let samples = stream.after(Some(Instant::now())).samples;
        assert_eq!(
            samples.iter().map(|sample| sample.time.unwrap()).collect::<Vec<_>>(),
            [43.0, 44.0, 342.0, 343.0],
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_snapshot_loads_hardware_and_history_seeds() {
        let source = Source::Mock(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"));
        let snapshot = source.current(&client(), &mut MockState::default(), &StreamMetrics::default()).await;
        let system = snapshot.system.unwrap();
        let cpu = system.cpu.unwrap();
        assert_eq!(cpu.per_core.unwrap().len(), 24);
        assert_eq!(cpu.freq_mhz, Some(3988.4));
        assert_eq!(system.net.unwrap()[0].iface, "eth0");
        assert_eq!(system.disk_io.unwrap()[0].dev, "nvme0n1");
        assert_eq!(snapshot.disks.unwrap().linux.unwrap()[0].mount.as_deref(), Some("/"));
        assert_eq!(snapshot.stream_samples.len(), 7);
        assert_eq!(snapshot.stream_samples.back().unwrap().time, system.time.map(|time| time - 2.0));
        let metrics = METRICS.into_iter().filter(|metric| !metric.starts_with("user_"));
        for metric in metrics {
            assert_eq!(snapshot.hardware_history[metric].points.last().map(|point| point.0), system.time);
        }
    }

    #[test]
    fn builds_json_mutations_and_encodes_fingerprints() {
        let (method, route, body) = Action::RemoveKey { user: "alice".into(), fingerprint: "SHA256:a/b+".into() }.request();
        assert_eq!((method, route, body), (Method::DELETE, "/users/alice/keys/SHA256%3Aa%2Fb%2B".into(), None));
        let action = Action::AddUser { user: "alice".into(), pubkey: "ssh-ed25519 TEST".into() };
        assert_eq!(action.request(), (Method::POST, "/users".into(), Some(serde_json::json!({"user": "alice", "pubkey": "ssh-ed25519 TEST"}))));
        let policy = Policy { mem_low_gb: 1, mem_high_gb: 2, total_max_gb: 3, cpu_weight: 100 };
        assert_eq!(Action::SetPolicy(policy).request(), (Method::PUT, "/policy".into(),
            Some(serde_json::json!({"mem_low_gb": 1, "mem_high_gb": 2, "total_max_gb": 3, "cpu_weight": 100}))));
    }

    #[test]
    fn npu_status_parses_live_daemon_payload() {
        let s: NpuStatus = serde_json::from_str(include_str!("../fixtures/npu-status-live.json")).unwrap();
        assert_eq!(s.up, Some(true));
        assert_eq!(s.devices.unwrap().len(), 4);
    }

    #[test]
    fn npu_status_accepts_missing_and_null_fields() {
        for json in [
            "{}",
            r#"{"up":null,"npu":null,"devices":null,"openvino":null,"cached_models":null,
                "active":null,"queued":null,"served":null,"errors":null,"avg_latency_s":null}"#,
        ] {
            let status: NpuStatus = serde_json::from_str(json).unwrap();
            assert!(status.up.is_none());
        }
        let status: NpuStatus = serde_json::from_str(r#"{
            "up":true,"npu":{"name":null,"npu_driver_version":null},
            "devices":["NPU","CPU"],"active":{"user":null},"queued":{"alice":null}
        }"#).unwrap();
        assert!(status.active.as_ref().and_then(NpuActive::user).is_none());
    }

    #[test]
    fn role_change_requests_encode_both_roles() {
        for (role, expected) in [(UserRole::Admin, "admin"), (UserRole::User, "user")] {
            assert_eq!(
                Action::SetUserRole { user: "a/b".into(), role }.request(),
                (Method::PUT, "/users/a%2Fb/role".into(), Some(serde_json::json!({"role": expected}))),
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_mutations_update_tables_and_audit() {
        let source = Source::Mock(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"));
        let http = client();
        let mut state = MockState::default();
        let initial = source.current(&http, &mut state, &StreamMetrics::default()).await;
        let identity = initial.identity.as_ref();
        let user = "charlie".to_owned();
        let initial_key = "ssh-ed25519 MOCK1 initial".to_owned();
        let added_key = "ssh-ed25519 MOCK2 second".to_owned();
        let policy = Policy { mem_low_gb: 1, mem_high_gb: 2, total_max_gb: 3, cpu_weight: 100 };
        for action in [
            Action::AddUser { user: user.clone(), pubkey: initial_key.clone() },
            Action::AddKey { user: user.clone(), pubkey: added_key },
            Action::RemoveKey { user: user.clone(), fingerprint: mock_key(&initial_key).unwrap().fingerprint },
            Action::Stop { id: "bob-23456".into(), owner: "bob".into() },
            Action::SetPolicy(policy.clone()),
        ] {
            source.mutate(&http, &mut state, action).await.unwrap();
        }
        let current = source.current(&http, &mut state, &StreamMetrics::default()).await;
        assert_eq!(current.users.iter().find(|entry| entry.user == user).unwrap().keys, Some(1));
        assert_eq!(current.kernels.len(), initial.kernels.len() - 1);
        let Detail::Keys(keys) = source.detail(&http, &mut state, &View::Keys(user.clone()), identity).await.unwrap() else {
            panic!("missing keys");
        };
        assert_eq!(keys[0].comment, "second");
        let Detail::Policy(updated) = source.detail(&http, &mut state, &View::Policy, identity).await.unwrap() else {
            panic!("missing policy");
        };
        assert_eq!(updated.total_max_gb, Some(policy.total_max_gb));
        source.mutate(&http, &mut state, Action::Disable(user.clone())).await.unwrap();
        assert!(!source.current(&http, &mut state, &StreamMetrics::default()).await.users.iter().any(|entry| entry.user == user));
        let Detail::Audit(log) = source.detail(&http, &mut state, &View::Audit, identity).await.unwrap() else {
            panic!("missing audit");
        };
        assert_eq!(log.len(), 8);
        assert!(log[..6].iter().all(|entry| entry.ok));
        let pending: Audit = serde_json::from_str(r#"{"time":1,"actor":"a","action":"user-add","target":"b","ok":null}"#).unwrap();
        assert!(!pending.ok);
        assert_eq!(log[0].action, "user-disable");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_history_filters_by_user_and_range() {
        let source = Source::Mock(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"));
        let mut state = MockState::default();
        let http = client();
        let identity = Identity { user: "alice".into(), role: "admin".into() };
        let view = |user: &str, range| View::History { users: Vec::new(), user: Some(user.into()), range };
        let pick = |detail: Detail| match detail {
            Detail::History(all, _) => all.into_iter().find(|(m, _)| *m == "user_mem").unwrap().1,
            _ => panic!("missing history"),
        };
        let alice = pick(source.detail(&http, &mut state, &view("alice", 0), Some(&identity)).await.unwrap()).unwrap();
        let bob = pick(source.detail(&http, &mut state, &view("bob", 3), Some(&identity)).await.unwrap()).unwrap();
        assert!(bob.points.len() > alice.points.len());
        assert_ne!(alice.points.last(), bob.points.last());
        let regular = Identity { user: "alice".into(), role: "user".into() };
        assert!(pick(source.detail(&http, &mut state, &view("bob", 3), Some(&regular)).await.unwrap()).is_err());
    }

    #[test]
    fn encodes_kernel_ids_as_one_path_segment() {
        assert_eq!(encode_segment("alice-12345"), "alice-12345");
        assert_eq!(encode_segment("../a/b?c#d"), "%2E%2E%2Fa%2Fb%3Fc%23d");
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Policy {
    pub mem_low_gb: u64,
    pub mem_high_gb: u64,
    pub total_max_gb: u64,
    pub cpu_weight: u64,
}

#[derive(Clone, Deserialize)]
pub struct CurrentPolicy {
    pub mem_low_gb: Option<u64>,
    pub mem_high_gb: Option<u64>,
    pub total_max_gb: Option<u64>,
    pub cpu_weight: Option<u64>,
}

impl From<&Policy> for CurrentPolicy {
    fn from(policy: &Policy) -> Self {
        Self {
            mem_low_gb: Some(policy.mem_low_gb),
            mem_high_gb: Some(policy.mem_high_gb),
            total_max_gb: Some(policy.total_max_gb),
            cpu_weight: Some(policy.cpu_weight),
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct Key {
    pub fingerprint: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub comment: String,
}

#[derive(Deserialize)]
pub struct Env {
    pub name: String,
    pub path: String,
    pub python: Option<String>,
    pub spyder_kernels: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct Audit {
    pub time: f64,
    pub actor: String,
    pub action: String,
    pub target: String,
    #[serde(default, deserialize_with = "null_as_false")]
    pub ok: bool,
    pub detail: Option<String>,
}

fn null_as_false<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(d)?.unwrap_or(false))
}

pub const METRICS: [&str; 11] = [
    "cpu_percent", "mem_used", "gpu_mem_used", "gpu_util", "user_mem", "user_cpu",
    "net_rx", "net_tx", "disk_read", "disk_write", "gpu_temp",
];
pub const LIVE_WINDOW: u64 = 300;
pub const RANGES: [(&str, u64, u64); 4] = [("1h", 3600, 10), ("6h", 21600, 60), ("24h", 86400, 240), ("7d", 604800, 1800)];

#[derive(Clone, Deserialize)]
pub struct Hardware {
    pub cpu: Option<HardwareCpu>,
    pub memory: Option<HardwareMemory>,
    pub gpus: Option<Vec<HardwareGpu>>,
    pub board: Option<String>,
    pub os: Option<String>,
    pub host_boot: Option<serde_json::Value>,
    pub disks: Option<Disks>,
}

#[derive(Clone, Deserialize)]
pub struct HardwareCpu {
    pub model: Option<String>,
    pub logical: Option<u64>,
    pub wsl_processors: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct HardwareMemory {
    pub host_total: Option<u64>,
    pub wsl_total: Option<u64>,
    pub dimms: Option<serde_json::Value>,
    pub speed_mts: Option<u64>,
    pub user_slice_max: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct HardwareGpu {
    pub index: Option<u64>,
    pub name: Option<String>,
    pub mem_total: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct Disks {
    pub linux: Option<Vec<Volume>>,
    pub windows: Option<Vec<Volume>>,
    pub physical: Option<Vec<PhysicalDisk>>,
}

#[derive(Clone, Deserialize)]
pub struct Volume {
    pub mount: Option<String>,
    pub total: Option<u64>,
    pub used: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct PhysicalDisk {
    pub name: Option<String>,
    pub media: Option<String>,
    pub size: Option<u64>,
}

#[derive(Clone, Deserialize)]
pub struct Wsl {
    pub wsl_version: Option<String>,
    pub kernel: Option<String>,
    pub windows: Option<String>,
    pub distros: Option<Vec<Distro>>,
    pub autostart_task: Option<String>,
    pub config: Option<WslConfig>,
    pub restart_pending: Option<bool>,
}

#[derive(Clone, Deserialize)]
pub struct Distro {
    pub name: Option<String>,
    pub state: Option<String>,
    #[serde(default, deserialize_with = "optional_integer")]
    pub version: Option<u64>,
    pub default: Option<bool>,
}

#[derive(Clone, Deserialize, Default)]
pub struct WslConfig {
    pub path: Option<String>,
    pub memory_gb: Option<u64>,
    pub processors: Option<u64>,
    pub swap_gb: Option<u64>,
    pub networking_mode: Option<String>,
    #[serde(default, deserialize_with = "optional_integer")]
    pub vm_idle_timeout: Option<i64>,
}

fn optional_integer<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr + Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number<T> { Number(T), Text(String) }
    Ok(match Option::<Number<T>>::deserialize(deserializer)? {
        Some(Number::Number(number)) => Some(number),
        Some(Number::Text(text)) => text.trim().parse().ok(),
        None => None,
    })
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WslEdit {
    pub memory_gb: u64,
    pub processors: u64,
    pub swap_gb: u64,
}

pub struct Overview {
    pub policy: Option<CurrentPolicy>,
    pub audit: Vec<Audit>,
    pub restart_pending: bool,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    None,
    Overview,
    Hardware,
    Wsl,
    Keys(String),
    Policy,
    History { users: Vec<String>, user: Option<String>, range: usize },
    Envs,
    Audit,
}

#[derive(Clone)]
pub struct UsageRank { pub user: String, pub cpu_avg: f64, pub cpu_peak: f64, pub mem_avg: f64, pub mem_peak: f64 }

pub enum Detail {
    None,
    Overview(Overview),
    Hardware(Hardware),
    Wsl(Wsl, Option<u64>),
    Keys(Vec<Key>),
    Policy(CurrentPolicy),
    History(Vec<(&'static str, Result<History>)>, Vec<UsageRank>),
    Envs(Vec<Env>),
    Audit(Vec<Audit>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Stop { id: String, owner: String },
    AddUser { user: String, pubkey: String },
    Disable(String),
    SetUserRole { user: String, role: UserRole },
    AddKey { user: String, pubkey: String },
    RemoveKey { user: String, fingerprint: String },
    SetPolicy(Policy),
    SetWsl(WslEdit),
    RestartWsl,
}

impl Action {
    fn audit_fields(&self) -> (&str, &str) {
        match self {
            Self::Stop { id, .. } => ("kernel-stop", id),
            Self::AddUser { user, .. } => ("user-add", user),
            Self::Disable(user) => ("user-disable", user),
            Self::SetUserRole { user, .. } => ("user-role", user),
            Self::AddKey { user, .. } => ("key-add", user),
            Self::RemoveKey { user, .. } => ("key-remove", user),
            Self::SetWsl(_) => ("wslconfig-set", "WSL"),
            Self::RestartWsl => ("wsl-restart", "WSL"),
            Self::SetPolicy(_) => ("policy-set", "policy"),
        }
    }

    fn request(&self) -> (Method, String, Option<serde_json::Value>) {
        use serde_json::json;
        match self {
            Self::Stop { id, .. } => (Method::DELETE, format!("/kernels/{}", encode_segment(id)), None),
            Self::AddUser { user, pubkey } => (Method::POST, "/users".into(), Some(json!({"user": user, "pubkey": pubkey}))),
            Self::Disable(user) => (Method::DELETE, format!("/users/{}", encode_segment(user)), None),
            Self::SetUserRole { user, role } => (
                Method::PUT, format!("/users/{}/role", encode_segment(user)),
                Some(json!({"role": role.as_str()})),
            ),
            Self::AddKey { user, pubkey } => (Method::POST, keys_route(user), Some(json!({"pubkey": pubkey}))),
            Self::RemoveKey { user, fingerprint } => (Method::DELETE, format!("{}/{}", keys_route(user), encode_segment(fingerprint)), None),
            Self::SetWsl(config) => (Method::PUT, "/wsl/config".into(), Some(json!(config))),
            Self::RestartWsl => (Method::POST, "/wsl/restart".into(), None),
            Self::SetPolicy(policy) => (Method::PUT, "/policy".into(), Some(json!(policy))),
        }
    }
}

fn keys_route(user: &str) -> String {
    format!("/users/{}/keys", encode_segment(user))
}

pub fn admin(identity: Option<&Identity>) -> bool {
    identity.is_some_and(|me| me.role == "admin")
}

fn scope(data: &mut Snapshot) {
    if !admin(data.identity.as_ref()) {
        data.users.retain(|user| data.identity.as_ref().is_some_and(|me| me.role == "user" && me.user == user.user));
        data.kernels.retain(|kernel| can_stop(data.identity.as_ref(), &kernel.user));
    }
}

fn epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[derive(Default)]
pub struct MockState {
    snapshot: Option<Snapshot>,
    hardware_history: Arc<BTreeMap<&'static str, History>>,
    next_history: Option<Instant>,
    disks: Option<Disks>,
    next_disks: Option<Instant>,
    npu: Option<NpuStatus>,
    pub next_npu: Option<Instant>,
    keys: Option<std::collections::BTreeMap<String, Vec<Key>>>,
    policy: Option<Policy>,
    wsl: Option<Wsl>,
    audit: Option<Vec<Audit>>,
    audit_updates: Vec<Audit>,
}

impl Source {
    pub async fn current(&self, http: &HttpClient, mock: &mut MockState, stream: &StreamMetrics) -> Snapshot {
        let now = Instant::now();
        if let Some(mut snapshot) = mock.snapshot.clone() {
            self.refresh_npu(http, &mut mock.npu, &mut mock.next_npu, now).await;
            snapshot.npu = mock.npu.clone();
            return snapshot;
        }
        let (mut snapshot, ()) = tokio::join!(
            self.snapshot(http, stream),
            self.refresh_hardware_preview(http, mock, now),
        );
        self.refresh_npu(http, &mut mock.npu, &mut mock.next_npu, now).await;
        snapshot.npu = mock.npu.clone();
        snapshot.hardware_history = Arc::clone(&mock.hardware_history);
        snapshot.disks = mock.disks.clone();
        if matches!(self, Self::Mock(_)) {
            if let Ok(samples) = self.get(http, "", "stream-system.json").await {
                snapshot.stream_samples = Arc::new(samples);
            }
            mock.snapshot = Some(snapshot.clone());
        }
        snapshot
    }

    async fn refresh_npu(
        &self, http: &HttpClient, status: &mut Option<NpuStatus>, next_poll: &mut Option<Instant>, now: Instant,
    ) {
        if next_poll.is_none_or(|next| now >= next) {
            *next_poll = Some(now + Duration::from_secs(5));
            *status = self.get(http, "/npu/status", "npu-status.json").await.unwrap_or_default();
        }
    }

    async fn refresh_hardware_preview(&self, http: &HttpClient, state: &mut MockState, now: Instant) {
        let history = async {
            if state.next_history.is_some_and(|next| next > now) {
                return;
            }
            state.next_history = Some(now + Duration::from_secs(30));
            let since = epoch().saturating_sub(LIVE_WINDOW);
            let mut requests = tokio::task::JoinSet::new();
            for metric in METRICS.into_iter().filter(|metric| !metric.starts_with("user_")) {
                if state.hardware_history.contains_key(metric) {
                    continue;
                }
                let source = self.clone();
                let http = http.clone();
                requests.spawn(async move {
                    let route = format!("/history?metric={metric}&since={since}&step=10");
                    let fixture = format!("history-{metric}.json");
                    (metric, source.get::<History>(&http, &route, &fixture).await)
                });
            }
            while let Some(result) = requests.join_next().await {
                if let Ok((metric, Ok(history))) = result {
                    Arc::make_mut(&mut state.hardware_history).insert(metric, history);
                }
            }
        };
        let disks = async {
            if state.next_disks.is_some_and(|next| next > now) {
                return;
            }
            state.next_disks = Some(now + Duration::from_secs(30));
            if let Ok(hardware) = self.get::<Hardware>(http, "/hardware", "hardware.json").await {
                state.disks = hardware.disks;
            }
        };
        tokio::join!(history, disks, self.refresh_npu(http, &mut state.npu, &mut state.next_npu, now));
    }

    async fn mock_keys<'a>(&self, http: &HttpClient, mock: &'a mut MockState) -> Result<&'a mut std::collections::BTreeMap<String, Vec<Key>>> {
        if mock.keys.is_none() {
            mock.keys = Some(self.get(http, "", "users-keys.json").await?);
        }
        Ok(mock.keys.as_mut().unwrap())
    }

    async fn wsl(&self, http: &HttpClient, mock: &MockState) -> Result<Wsl> {
        match &mock.wsl {
            Some(wsl) => Ok(wsl.clone()),
            None => self.get(http, "/wsl", "wsl.json").await,
        }
    }

    async fn policy(&self, http: &HttpClient, mock: &MockState) -> Result<CurrentPolicy> {
        match &mock.policy {
            Some(policy) => Ok(policy.into()),
            None => self.get(http, "/policy", "policy.json").await,
        }
    }

    async fn audit(&self, http: &HttpClient, mock: &mut MockState) -> Result<Vec<Audit>> {
        let mut entries: Vec<Audit> = match self {
            Self::Socket(_) => self.get(http, "/audit?limit=100", "").await?,
            Self::Mock(_) => {
                if mock.audit.is_none() {
                    mock.audit = Some(self.get(http, "", "audit.json").await?);
                }
                mock.audit_updates.iter().rev().chain(mock.audit.iter().flatten()).take(100).cloned().collect()
            }
        };
        entries.sort_by(|left, right| right.time.total_cmp(&left.time));
        Ok(entries)
    }

    async fn history_one(&self, http: &HttpClient, metric: &'static str, user: Option<&str>, range: usize, identity: Option<&Identity>, is_mock: bool) -> Result<History> {
        let (_, seconds, step) = RANGES[range];
        let end = epoch();
        let mut route = format!("/history?metric={metric}&since={}&step={step}", end.saturating_sub(seconds));
        let user = if metric.starts_with("user_") {
            let user = user.ok_or("No user available for this metric")?;
            if !can_stop(identity, user) {
                return Err("History access denied".into());
            }
            route.push_str(&format!("&user={}", encode_segment(user)));
            Some(user)
        } else {
            None
        };
        let fixture = format!("history-{metric}.json");
        let mut history: History = match (is_mock, user) {
            (true, Some(user)) => {
                let histories: std::collections::BTreeMap<String, History> = self.get(http, "", &fixture).await?;
                histories.get(user).cloned().unwrap_or(History { points: Vec::new(), window: None })
            }
            _ => self.get(http, &route, &fixture).await?,
        };
        let end = if is_mock {
            history.points.last().map_or(end as f64, |(time, _)| *time)
        } else {
            end as f64
        };
        let start = end - seconds as f64;
        history.window = Some([start, end]);
        if is_mock {
            history.points.retain(|(time, _)| *time >= start);
        }
        Ok(history)
    }

    pub async fn detail(&self, http: &HttpClient, mock: &mut MockState, view: &View, identity: Option<&Identity>) -> Result<Detail> {
        let is_mock = matches!(self, Self::Mock(_));
        if matches!(view, View::Keys(_) | View::Policy | View::Audit | View::Wsl) && !admin(identity) {
            return Err("Admin role required".into());
        }
        match view {
            View::None => Ok(Detail::None),
            View::Keys(user) if is_mock => Ok(Detail::Keys(self.mock_keys(http, mock).await?.get(user).cloned().unwrap_or_default())),
            View::Keys(user) => self.get(http, &keys_route(user), "").await.map(Detail::Keys),
            View::Overview => {
                let mut overview = Overview { policy: None, audit: Vec::new(), restart_pending: false, errors: Vec::new() };
                if admin(identity) {
                    let (policy, wsl) = tokio::join!(self.policy(http, mock), self.wsl(http, mock));
                    overview.policy = record(policy, "/policy", &mut overview.errors);
                    overview.restart_pending = record(wsl, "/wsl", &mut overview.errors)
                        .and_then(|wsl| wsl.restart_pending).unwrap_or(false);
                    overview.audit = record(self.audit(http, mock).await, "/audit", &mut overview.errors).unwrap_or_default();
                }
                Ok(Detail::Overview(overview))
            }
            View::Hardware => self.get(http, "/hardware", "hardware.json").await.map(Detail::Hardware),
            View::Wsl => {
                let (wsl, hardware) = tokio::join!(self.wsl(http, mock), self.get::<Hardware>(http, "/hardware", "hardware.json"));
                Ok(Detail::Wsl(wsl?, hardware.ok().and_then(|hw| hw.cpu?.logical)))
            }
            View::Policy => self.policy(http, mock).await.map(Detail::Policy),
            View::Audit => self.audit(http, mock).await.map(Detail::Audit),
            View::Envs => self.get(http, "/envs", "envs.json").await.map(Detail::Envs),
            View::History { users, user, range } => {
                let mut all = Vec::with_capacity(METRICS.len());
                for metric in METRICS {
                    all.push((metric, self.history_one(http, metric, user.as_deref(), *range, identity, is_mock).await));
                }
                let mut board = Vec::new();
                for name in users {
                    let (cpu, mem) = tokio::join!(
                        self.history_one(http, "user_cpu", Some(name), *range, identity, is_mock),
                        self.history_one(http, "user_mem", Some(name), *range, identity, is_mock));
                    let values = |h: Result<History>| h.map(|h| h.points.into_iter().filter_map(|(_, v)| v.filter(|v| v.is_finite())).collect::<Vec<_>>()).unwrap_or_default();
                    let (cpu, mem) = (values(cpu), values(mem));
                    if cpu.is_empty() && mem.is_empty() { continue; }
                    let avg = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
                    board.push(UsageRank {
                        user: name.clone(), cpu_avg: avg(&cpu), cpu_peak: cpu.iter().copied().fold(0.0, f64::max),
                        mem_avg: avg(&mem), mem_peak: mem.iter().copied().fold(0.0, f64::max),
                    });
                }
                board.sort_by(|a, b| b.cpu_avg.total_cmp(&a.cpu_avg));
                Ok(Detail::History(all, board))
            }
        }
    }

    pub async fn mutate(&self, http: &HttpClient, mock: &mut MockState, action: Action) -> Result<String> {
        let identity = if let Some(snapshot) = &mock.snapshot {
            snapshot.identity.clone().ok_or("Identity unavailable")?
        } else {
            self.get(http, "/auth/me", "auth-me.json").await?
        };
        let allowed = match &action {
            Action::Stop { owner, .. } => can_stop(Some(&identity), owner),
            _ => admin(Some(&identity)),
        };
        if !allowed {
            return Err("Current role cannot perform this action".into());
        }
        let (verb, target) = action.audit_fields();
        if matches!(self, Self::Mock(_)) {
            let verb = verb.to_owned();
            let target = target.to_owned();
            let result = self.mock_mutation(http, mock, action).await;
            mock.audit_updates.push(Audit { time: epoch() as f64, actor: identity.user, action: verb.clone(), target: target.clone(), ok: result.is_ok(), detail: result.as_ref().err().cloned() });
            result?;
            Ok(format!("{verb} {target} succeeded (mock)"))
        } else {
            let (method, route, body) = action.request();
            self.request(http, method, &route, body).await?;
            Ok(format!("{verb} {target} succeeded"))
        }
    }

    async fn mock_mutation(&self, http: &HttpClient, mock: &mut MockState, action: Action) -> Result<()> {
        match action {
            Action::Stop { id, owner } => {
                let data = mock.snapshot.as_mut().ok_or("Snapshot unavailable")?;
                let index = data.kernels.iter().position(|kernel| kernel.id == id && kernel.user == owner).ok_or("Kernel not found")?;
                data.kernels.remove(index);
                if let Some(user) = data.users.iter_mut().find(|user| user.user == owner) {
                    user.kernels = user.kernels.map(|count| count.saturating_sub(1));
                }
            }
            Action::AddUser { user, pubkey } => {
                let key = mock_key(&pubkey)?;
                let data = mock.snapshot.as_ref().ok_or("Snapshot unavailable")?;
                if data.users.iter().any(|entry| entry.user == user) {
                    return Err("User already exists".into());
                }
                let uid = data.users.iter().filter_map(|entry| entry.uid).max().unwrap_or(1000).saturating_add(1);
                self.mock_keys(http, mock).await?.insert(user.clone(), vec![key]);
                if let Some(data) = &mut mock.snapshot {
                    data.users.push(User { user, uid: Some(uid), role: Some("user".into()), linger: Some(true), keys: Some(1), sessions: Some(0), kernels: Some(0), usage: None });
                }
            }
            Action::Disable(user) => {
                let data = mock.snapshot.as_mut().ok_or("Snapshot unavailable")?;
                let index = data.users.iter().position(|entry| entry.user == user).ok_or("User not found")?;
                if data.users[index].role.as_deref() == Some("admin") || data.identity.as_ref().is_some_and(|me| me.user == user) {
                    return Err("Protected account".into());
                }
                let keys = self.mock_keys(http, mock).await?;
                keys.remove(&user);
                if let Some(data) = &mut mock.snapshot {
                    data.users.remove(index);
                    data.kernels.retain(|kernel| kernel.user != user);
                }
            }
            Action::SetUserRole { user, role } => {
                let data = mock.snapshot.as_mut().ok_or("Snapshot unavailable")?;
                let entry = data.users.iter_mut().find(|entry| entry.user == user).ok_or("User not found")?;
                entry.role = Some(role.as_str().into());
                if let Some(me) = data.identity.as_mut().filter(|me| me.user == user) {
                    me.role = role.as_str().into();
                }
                scope(data);
            }
            Action::AddKey { user, pubkey } => {
                if !mock.snapshot.as_ref().is_some_and(|data| data.users.iter().any(|entry| entry.user == user)) {
                    return Err("User not found".into());
                }
                let key = mock_key(&pubkey)?;
                let keys = self.mock_keys(http, mock).await?;
                let keys = keys.entry(user.clone()).or_default();
                if keys.iter().any(|existing| existing.fingerprint == key.fingerprint) {
                    return Err("Key already exists".into());
                }
                keys.push(key);
                mock_key_count(mock, &user, 1);
            }
            Action::RemoveKey { user, fingerprint } => {
                let keys = self.mock_keys(http, mock).await?.get_mut(&user).ok_or("Key not found")?;
                let index = keys.iter().position(|key| key.fingerprint == fingerprint).ok_or("Key not found")?;
                keys.remove(index);
                mock_key_count(mock, &user, -1);
            }
            Action::SetPolicy(policy) => mock.policy = Some(policy),
            Action::SetWsl(edit) => {
                let mut wsl = self.wsl(http, mock).await?;
                let config = wsl.config.get_or_insert_with(WslConfig::default);
                config.memory_gb = Some(edit.memory_gb);
                config.processors = Some(edit.processors);
                config.swap_gb = Some(edit.swap_gb);
                wsl.restart_pending = Some(true);
                mock.wsl = Some(wsl);
            }
            Action::RestartWsl => {
                let mut wsl = self.wsl(http, mock).await?;
                wsl.restart_pending = Some(false);
                mock.wsl = Some(wsl);
                if let Some(data) = &mut mock.snapshot {
                    data.kernels.clear();
                    for user in &mut data.users {
                        user.kernels = Some(0);
                    }
                }
            }
        }
        Ok(())
    }
}

fn mock_key(pubkey: &str) -> Result<Key> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut parts = pubkey.split_whitespace();
    let kind = parts.next().ok_or("Public key required")?;
    let blob = parts.next().ok_or("Public key data required")?;
    let mut hash = DefaultHasher::new();
    blob.hash(&mut hash);
    Ok(Key { fingerprint: format!("mock:{:016x}", hash.finish()), kind: kind.into(), comment: parts.collect::<Vec<_>>().join(" ") })
}

fn mock_key_count(mock: &mut MockState, user: &str, delta: i64) {
    if let Some(entry) = mock.snapshot.as_mut().and_then(|data| data.users.iter_mut().find(|entry| entry.user == user)) {
        entry.keys = Some(entry.keys.unwrap_or_default().saturating_add_signed(delta));
    }
}

#[derive(Clone, Default)]
pub struct StreamMetrics {
    samples: Arc<VecDeque<System>>,
    system: Option<(std::time::Instant, System)>,
    users: Option<(std::time::Instant, Vec<User>)>,
    kernels: Option<(std::time::Instant, Vec<Kernel>)>,
}

impl StreamMetrics {
    pub fn after(mut self, cutoff: Option<std::time::Instant>) -> Self {
        if let Some(cutoff) = cutoff {
            self.system = self.system.filter(|(time, _)| *time > cutoff);
            self.users = self.users.filter(|(time, _)| *time > cutoff);
            self.kernels = self.kernels.filter(|(time, _)| *time > cutoff);
        }
        self
    }

    fn event(&mut self, name: &str, json: &str) -> bool {
        let now = std::time::Instant::now();
        fn update<T: DeserializeOwned>(slot: &mut Option<(std::time::Instant, T)>, now: std::time::Instant, json: &str) -> bool {
            let Ok(value) = serde_json::from_str(json) else { return false; };
            *slot = Some((now, value));
            true
        }
        match name {
            "system" => {
                if !update(&mut self.system, now, json) {
                    return false;
                }
                let (_, sample) = self.system.as_ref().expect("updated system sample");
                if let Some(time) = sample.time {
                    let samples = Arc::make_mut(&mut self.samples);
                    if samples.back().and_then(|last| last.time).is_some_and(|last| last > time) {
                        samples.clear();
                    }
                    if samples.back().and_then(|last| last.time) != Some(time) {
                        samples.push_back(sample.clone());
                    }
                    while samples.len() > 512 || samples.front().and_then(|first| first.time)
                        .is_some_and(|first| first < time - LIVE_WINDOW as f64)
                    {
                        samples.pop_front();
                    }
                }
                true
            }
            "users" => update(&mut self.users, now, json),
            "kernels" => update(&mut self.kernels, now, json),
            _ => false,
        }
    }
}

#[derive(Default)]
struct SseParser {
    pending: Vec<u8>,
    name: String,
    data: String,
}

impl SseParser {
    fn push(&mut self, chunk: &[u8], mut dispatch: impl FnMut(&str, &str)) -> Result<()> {
        const LIMIT: usize = 8 * 1024 * 1024;
        self.pending.extend_from_slice(chunk);
        let mut consumed = 0;
        for raw in self.pending.split_inclusive(|&byte| byte == b'\n') {
            let Some(line) = raw.strip_suffix(b"\n") else { break; };
            let line = std::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line)).map_err(|e| e.to_string())?;
            consumed += raw.len();
            if line.is_empty() {
                if !self.data.is_empty() {
                    dispatch(&self.name, &self.data);
                }
                self.name.clear();
                self.data.clear();
            } else if let Some((field, value)) = line.split_once(':') {
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => self.name = value.to_owned(),
                    "data" => {
                        self.data.push_str(value);
                        self.data.push('\n');
                    }
                    _ => {}
                }
            }
            if self.name.len() + self.data.len() > LIMIT {
                return Err("Stream event is too large".into());
            }
        }
        self.pending.drain(..consumed);
        if self.pending.len() > LIMIT {
            return Err("Stream line is too large".into());
        }
        Ok(())
    }
}

async fn stream_once(http: &HttpClient, uri: &hyper::Uri, sender: &tokio::sync::watch::Sender<StreamMetrics>) -> Result<()> {
    let request = Request::get(uri.clone())
        .header("host", "localhost")
        .header("accept", "text/event-stream")
        .body(Full::new(Bytes::new())).map_err(|e| e.to_string())?;
    let response = tokio::time::timeout(Duration::from_secs(2), http.request(request)).await
        .map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let mut body = response.into_body();
    let mut parser = SseParser::default();
    while let Some(frame) = tokio::time::timeout(Duration::from_secs(8), body.frame()).await.map_err(|e| e.to_string())? {
        let frame = frame.map_err(|e| e.to_string())?;
        if let Ok(data) = frame.into_data() {
            parser.push(&data, |name, json| {
                sender.send_if_modified(|metrics| metrics.event(name, json));
            })?;
        }
    }
    Ok(())
}
