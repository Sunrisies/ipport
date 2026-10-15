use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(address: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn web_api_reads_ports_and_rejects_invalid_filters() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_ipport"))
            .args(["web", "--bind", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(server.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let address = line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .strip_prefix("http://")
        .unwrap();

    let response = request(address, &format!("/api/ports?port={port}&protocol=tcp"));
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(body).unwrap();
    let ports = snapshot["ports"].as_array().unwrap();
    assert!(!ports.is_empty());
    assert!(
        ports
            .iter()
            .all(|p| p["local_port"] == port && p["protocol"] == "tcp")
    );

    for path in [
        "/api/ports?port=65536",
        "/api/ports?protocol=http",
        "/api/ports?all=maybe",
    ] {
        assert!(request(address, path).starts_with("HTTP/1.1 400"));
    }
}
