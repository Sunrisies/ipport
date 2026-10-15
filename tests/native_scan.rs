use std::net::{TcpListener, TcpStream, UdpSocket};

use ipport::scanner::{Filter, Protocol, scan};

#[test]
fn finds_real_tcp_and_udp_ports_and_owning_process() {
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let snapshot = scan(&Filter::default()).unwrap();
    for (port, protocol, state) in [
        (tcp.local_addr().unwrap().port(), Protocol::Tcp, "LISTEN"),
        (udp.local_addr().unwrap().port(), Protocol::Udp, "BOUND"),
    ] {
        let info = snapshot
            .ports
            .iter()
            .find(|p| p.local_port == port && p.protocol == protocol)
            .unwrap_or_else(|| panic!("Missing {protocol} port {port}"));
        assert_eq!(info.state, state);
        assert_eq!(info.exposure, "loopback");
        assert!(
            info.processes.iter().any(|p| p.pid == std::process::id()),
            "Socket not attributed to this process: {info:?}"
        );
    }
}

#[test]
fn all_mode_includes_established_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_server, _) = listener.accept().unwrap();
    let port = listener.local_addr().unwrap().port();
    let filtered = scan(&Filter {
        port: Some(port),
        ..Filter::default()
    })
    .unwrap();
    assert!(filtered.ports.iter().all(|p| p.state == "LISTEN"));
    let all = scan(&Filter {
        port: Some(port),
        all: true,
        ..Filter::default()
    })
    .unwrap();
    assert!(
        all.ports
            .iter()
            .any(|p| p.state == "ESTABLISHED" && p.remote_endpoint.is_some())
    );
}

#[test]
fn formats_ipv6_listener_unambiguously() {
    let listener = TcpListener::bind("[::1]:0").unwrap();
    let address = listener.local_addr().unwrap();
    let snapshot = scan(&Filter {
        port: Some(address.port()),
        ..Filter::default()
    })
    .unwrap();
    let info = snapshot
        .ports
        .iter()
        .find(|p| p.local_endpoint == address.to_string())
        .unwrap();
    assert_eq!(info.family, "IPv6");
    assert_eq!(info.exposure, "loopback");
}
