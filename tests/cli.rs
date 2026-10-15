use std::{net::TcpListener, process::Command};

#[test]
fn cli_json_can_find_a_real_port() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let output = Command::new(env!("CARGO_BIN_EXE_ipport"))
        .args([
            "list",
            "--port",
            &port.to_string(),
            "--protocol",
            "tcp",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let ports = snapshot["ports"].as_array().unwrap();
    assert!(!ports.is_empty());
    assert!(
        ports
            .iter()
            .all(|p| p["local_port"] == port && p["protocol"] == "tcp")
    );
}

#[test]
fn rejects_invalid_port_and_public_web_binding() {
    let invalid = Command::new(env!("CARGO_BIN_EXE_ipport"))
        .args(["show", "65536"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    let public = Command::new(env!("CARGO_BIN_EXE_ipport"))
        .args(["web", "--bind", "0.0.0.0:8787"])
        .output()
        .unwrap();
    assert!(!public.status.success());
    assert!(String::from_utf8_lossy(&public.stderr).contains("loopback"));
}
