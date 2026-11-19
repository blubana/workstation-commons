use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::{Path, PathBuf}};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Console,
    Dashboard,
    Budget,
}

impl Layout {
    pub const ALL: [Self; 3] = [Self::Console, Self::Dashboard, Self::Budget];

    pub fn name(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Dashboard => "dashboard",
            Self::Budget => "budget",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.name() == value)
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path).ok().as_deref().and_then(|text| {
            assignments(text, "").find_map(|(key, value)| {
                (key == "layout").then(|| Self::parse(value.trim_matches(['"', '\'']))).flatten()
            })
        }).unwrap_or_default()
    }

    pub fn save(self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("layout = \"{}\"\n", self.name()))
    }
}

pub fn state_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/cm-top/state.toml"))
}

pub fn hosts() -> Vec<String> {
    fs::read_to_string("/etc/compute-mgr/tui.toml").ok()
        .and_then(|text| parse_hosts(&text))
        .unwrap_or_else(|| vec!["localhost".into()])
}

fn assignments(text: &str, section: &str) -> impl Iterator<Item = (String, String)> {
    let mut current = "";
    let mut entries = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in text.lines().map(|line| line.split('#').next().unwrap_or("").trim()) {
        if let Some((_, value)) = &mut pending {
            value.push_str(line);
            if line.ends_with(']') {
                entries.extend(pending.take());
            }
        } else if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            current = name.trim();
        } else if current == section {
            if let Some((key, value)) = line.split_once('=') {
                let entry = (key.trim().to_owned(), value.trim().to_owned());
                if entry.1.starts_with('[') && !entry.1.ends_with(']') {
                    pending = Some(entry);
                } else {
                    entries.push(entry);
                }
            }
        }
    }
    entries.into_iter()
}

fn parse_hosts(text: &str) -> Option<Vec<String>> {
    let (_, value) = assignments(text, "connect").find(|(key, _)| key == "hosts")?;
    let items = value.strip_prefix('[')?.strip_suffix(']')?.trim().trim_end_matches(',');
    let hosts: Vec<_> = items.split(',').map(|item| {
        let item = item.trim();
        let host = item.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
            .or_else(|| item.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))?;
        (!host.starts_with('-') && !host.is_empty() && host.bytes().all(|byte|
            byte.is_ascii_alphanumeric() || b".-_:[]".contains(&byte))).then(|| host.to_owned())
    }).collect::<Option<_>>()?;
    Some(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_persistence_round_trip() {
        // nosemgrep: rust.lang.security.temp-dir.temp-dir -- unit test fixture only
        let dir = env::temp_dir().join(format!("cm-top-state-{}", std::process::id()));
        let file = dir.join("config/state.toml");
        assert_eq!(Layout::load(&file), Layout::Console);
        for layout in Layout::ALL {
            layout.save(&file).unwrap();
            assert_eq!(Layout::load(&file), layout);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_connect_hosts() {
        assert_eq!(parse_hosts("[other]\nhosts = ['other']\n[connect]\nhosts = [\n'10.0.0.5', # LAN\n\"server.example.ts.net\",\n]"),
            Some(vec!["10.0.0.5".into(), "server.example.ts.net".into()]));
    }
}
