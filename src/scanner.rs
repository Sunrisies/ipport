use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use clap::ValueEnum;
use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sysinfo::Users;
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: Option<String>,
    pub executable: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortInfo {
    pub protocol: Protocol,
    pub family: &'static str,
    pub local_address: IpAddr,
    pub local_port: u16,
    pub local_endpoint: String,
    pub remote_endpoint: Option<String>,
    pub state: String,
    pub exposure: &'static str,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub port: Option<u16>,
    pub protocol: Option<Protocol>,
    pub pid: Option<u32>,
    pub search: Option<String>,
    pub all: bool,
}

impl Filter {
    pub fn matches(&self, port: &PortInfo) -> bool {
        if !self.all && port.protocol == Protocol::Tcp && port.state != "LISTEN" {
            return false;
        }
        if self.port.is_some_and(|value| value != port.local_port)
            || self.protocol.is_some_and(|value| value != port.protocol)
            || self
                .pid
                .is_some_and(|value| !port.processes.iter().any(|p| p.pid == value))
        {
            return false;
        }
        if let Some(search) = &self.search {
            let needle = search.trim().to_lowercase();
            let text = format!(
                "{} {} {} {} {} {}",
                port.local_endpoint,
                port.remote_endpoint.as_deref().unwrap_or_default(),
                port.protocol,
                port.state,
                port.family,
                port.processes
                    .iter()
                    .map(|p| format!(
                        "{} {} {} {}",
                        p.pid,
                        p.name.as_deref().unwrap_or_default(),
                        p.executable.as_deref().unwrap_or_default(),
                        p.user.as_deref().unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            return text.to_lowercase().contains(&needle);
        }
        true
    }
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub ports: Vec<PortInfo>,
    pub scanned_at_ms: u128,
    pub elapsed_ms: u128,
    pub platform: &'static str,
    pub warnings: Vec<String>,
}

/// Read native OS socket tables. Never shells out to a command-line utility.
pub fn scan(filter: &Filter) -> Result<Snapshot> {
    let started = Instant::now();
    let entries = netstat2::iterate_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP | ProtocolFlags::UDP,
    )
    .context("Cannot read the operating system's socket tables")?;
    // A process may exit between enumerating its descriptors and reading a socket.
    // Preserve readable entries rather than discarding the entire snapshot.
    let mut skipped = 0;
    let sockets: Vec<_> = entries
        .filter_map(|entry| match entry {
            Ok(socket) => Some(socket),
            Err(_) => {
                skipped += 1;
                None
            }
        })
        .collect();

    let pids: Vec<Pid> = sockets
        .iter()
        .flat_map(|s| s.associated_pids.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Pid::from_u32)
        .collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_exe(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    #[cfg(windows)]
    let users = Users::new_with_refreshed_list();
    let mut ports = Vec::new();
    for socket in sockets {
        let (protocol, local_address, local_port, remote_endpoint, state) =
            match socket.protocol_socket_info {
                ProtocolSocketInfo::Tcp(tcp) => (
                    Protocol::Tcp,
                    tcp.local_addr,
                    tcp.local_port,
                    (tcp.remote_port != 0)
                        .then(|| SocketAddr::new(tcp.remote_addr, tcp.remote_port).to_string()),
                    if tcp.state == TcpState::Listen {
                        "LISTEN".to_owned()
                    } else {
                        tcp.state.to_string()
                    },
                ),
                ProtocolSocketInfo::Udp(udp) => (
                    Protocol::Udp,
                    udp.local_addr,
                    udp.local_port,
                    None,
                    "BOUND".to_owned(),
                ),
            };
        // Unbound sockets are not allocated local ports.
        if local_port == 0 {
            continue;
        }
        let processes = socket
            .associated_pids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|pid| {
                let process = system.process(Pid::from_u32(pid));
                ProcessInfo {
                    pid,
                    name: process.map(|p| p.name().to_string_lossy().into_owned()),
                    executable: process
                        .and_then(|p| p.exe())
                        .map(|p| p.to_string_lossy().into_owned()),
                    user: process.and_then(|process| {
                        #[cfg(windows)]
                        {
                            process_user(process, &users)
                        }
                        #[cfg(not(windows))]
                        {
                            process_user(process)
                        }
                    }),
                }
            })
            .collect();
        let port = PortInfo {
            protocol,
            family: if local_address.is_ipv4() {
                "IPv4"
            } else {
                "IPv6"
            },
            local_address,
            local_port,
            local_endpoint: SocketAddr::new(local_address, local_port).to_string(),
            remote_endpoint,
            state,
            exposure: if local_address.is_loopback() {
                "loopback"
            } else if local_address.is_unspecified() {
                "all_interfaces"
            } else {
                "interface"
            },
            processes,
        };
        if filter.matches(&port) {
            ports.push(port);
        }
    }
    ports.sort_by(|a, b| {
        (
            a.local_port,
            a.protocol,
            &a.local_endpoint,
            &a.remote_endpoint,
        )
            .cmp(&(
                b.local_port,
                b.protocol,
                &b.local_endpoint,
                &b.remote_endpoint,
            ))
    });
    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} socket entries could not be read; results may be incomplete."
        ));
    }
    if ports
        .iter()
        .any(|p| p.processes.is_empty() || p.processes.iter().any(|p| p.name.is_none()))
    {
        warnings.push("Some process details are unavailable: permissions or a process exiting during the scan may prevent attribution.".to_owned());
    }
    Ok(Snapshot {
        ports,
        scanned_at_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        elapsed_ms: started.elapsed().as_millis(),
        platform: std::env::consts::OS,
        warnings,
    })
}

#[cfg(windows)]
fn process_user(process: &Process, users: &Users) -> Option<String> {
    process
        .user_id()
        .and_then(|id| users.get_user_by_id(id).map(|u| u.name().to_owned()))
}

#[cfg(target_os = "macos")]
fn process_user(process: &Process) -> Option<String> {
    use std::ffi::CStr;

    let uid = **process.user_id()?;
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0u8; 16 * 1024];
    let mut result = std::ptr::null_mut();
    // getpwuid_r is thread-safe and returns the same local or directory-backed account name.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    unsafe {
        CStr::from_ptr(entry.pw_name)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn process_user(_: &Process) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> PortInfo {
        PortInfo {
            protocol: Protocol::Tcp,
            family: "IPv6",
            local_address: "::1".parse().unwrap(),
            local_port: 8080,
            local_endpoint: "[::1]:8080".into(),
            remote_endpoint: Some("[::1]:9000".into()),
            state: "ESTABLISHED".into(),
            exposure: "loopback",
            processes: vec![ProcessInfo {
                pid: 123,
                name: Some("Example Worker".into()),
                executable: None,
                user: None,
            }],
        }
    }

    #[test]
    fn default_excludes_connections_but_keeps_bound_udp() {
        let mut port = connection();
        assert!(!Filter::default().matches(&port));
        port.protocol = Protocol::Udp;
        port.state = "BOUND".into();
        assert!(Filter::default().matches(&port));
    }

    #[test]
    fn filters_combine_and_search_is_case_insensitive() {
        let port = connection();
        let mut filter = Filter {
            all: true,
            port: Some(8080),
            protocol: Some(Protocol::Tcp),
            pid: Some(123),
            search: Some(" EXAMPLE ".into()),
        };
        assert!(filter.matches(&port));
        filter.pid = Some(456);
        assert!(!filter.matches(&port));
        filter.pid = None;
        filter.search = Some("[::1]:9000".into());
        assert!(filter.matches(&port));
        filter.port = Some(9000);
        assert!(!filter.matches(&port));
    }
}
