//! Signed storage requests must never inherit the Sculptary API credential.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Value};

const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct Expected {
    method: &'static str,
    target: &'static str,
    body: Vec<u8>,
    status: u16,
    response: Value,
    storage: bool,
}

fn api(method: &'static str, target: &'static str, body: Value, response: Value) -> Expected {
    Expected {
        method,
        target,
        body: if body.is_null() {
            vec![]
        } else {
            serde_json::to_vec(&body).unwrap()
        },
        status: 200,
        response,
        storage: false,
    }
}

struct Server {
    host: String,
    worker: JoinHandle<()>,
}
impl Server {
    fn new(build: impl FnOnce(&str) -> Vec<Expected>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let expected = build(&host);
        let worker = thread::spawn(move || {
            for expected in expected {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let (headers, body) = read_request(&mut stream);
                assert_eq!(
                    headers.lines().next().unwrap(),
                    format!("{} {} HTTP/1.1", expected.method, expected.target)
                );
                let headers = headers.to_ascii_lowercase();
                if expected.storage {
                    assert!(!headers.contains("authorization:"));
                    assert!(!headers.contains("cookie:"));
                    assert!(headers.contains("content-type: application/octet-stream"));
                    assert!(headers.contains("x-amz-checksum-sha256: signed-checksum"));
                    assert_eq!(body, expected.body);
                } else {
                    assert!(headers.contains("authorization: bearer test-token"));
                    assert_eq!(
                        if body.is_empty() {
                            Value::Null
                        } else {
                            serde_json::from_slice::<Value>(&body).unwrap()
                        },
                        if expected.body.is_empty() {
                            Value::Null
                        } else {
                            serde_json::from_slice::<Value>(&expected.body).unwrap()
                        }
                    );
                }
                let body = expected.response.to_string();
                write!(stream, "HTTP/1.1 {} Response\r\nContent-Type: application/json\r\nLocation: http://127.0.0.1:9/private-secret\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", expected.status, body.len(), body).unwrap();
            }
        });
        Self { host, worker }
    }
    fn run(&self, file: &std::path::Path) -> Output {
        let config = tempfile::tempdir().unwrap();
        std::fs::write(
            config.path().join("config.toml"),
            format!(
                "[hosts.\"{}\"]\nusername = \"alice\"\ntoken = \"test-token\"\n",
                self.host
            ),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_sculpt"))
            .args(["--host", &self.host, "release", "upload", "v1/kit"])
            .arg(file)
            .args(["--repo", "alice/castle", "--json"])
            .env("SCULPTARY_CONFIG", config.path().join("config.toml"))
            .env_remove("SCULPTARY_HOST")
            .output()
            .unwrap()
    }
    fn finish(self) {
        self.worker.join().unwrap();
    }
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut data = Vec::new();
    let mut buf = [0; 4096];
    let end = loop {
        let n = stream.read(&mut buf).unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&buf[..n]);
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let headers = String::from_utf8(data[..end].to_vec()).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap_or(0);
    while data.len() < end + length {
        let n = stream.read(&mut buf).unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&buf[..n]);
    }
    (headers, data[end..end + length].to_vec())
}

fn release(limit: u64) -> Value {
    json!({"id":"release-1", "asset_upload_max_bytes":limit})
}
fn prepared(storage: &str) -> Value {
    json!({"id":"asset-1", "upload":{"href":format!("{storage}/object?signature=private-token"),
        "headers":{"Content-Type":"application/octet-stream", "x-amz-checksum-sha256":"signed-checksum"}}})
}
fn lookup(limit: u64) -> Expected {
    api(
        "GET",
        "/api/v1/repos/alice/castle/releases/by-tag?tag=v1%2Fkit",
        Value::Null,
        release(limit),
    )
}
fn prepare(storage: &str) -> Expected {
    api(
        "POST",
        "/api/v1/repos/alice/castle/releases/release-1/assets",
        json!({"name":"kit.glb", "size":3, "sha256":SHA}),
        prepared(storage),
    )
}
fn storage(status: u16) -> Server {
    Server::new(|_| {
        vec![Expected {
            method: "PUT",
            target: "/object?signature=private-token",
            body: b"abc".to_vec(),
            status,
            response: json!({}),
            storage: true,
        }]
    })
}
fn file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kit.glb");
    std::fs::write(&path, b"abc").unwrap();
    (dir, path)
}

#[test]
fn release_upload_hashes_streams_and_confirms_without_forwarding_credentials() {
    let storage = storage(200);
    let server = Server::new(|_| {
        vec![
            lookup(512),
            prepare(&storage.host),
            api(
                "POST",
                "/api/v1/repos/alice/castle/releases/release-1/assets/asset-1/complete",
                json!({}),
                json!({"id":"asset-1", "name":"kit.glb", "size":3, "sha256":SHA, "status":"ready"}),
            ),
        ]
    });
    let (_dir, file) = file();
    let out = server.run(&file);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["sha256"], SHA);
    assert_eq!(value["status"], "ready");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("private-token"));
    server.finish();
    storage.finish();
}

#[test]
fn release_upload_rejects_oversized_files_before_preparing() {
    let server = Server::new(|_| vec![lookup(2)]);
    let (_dir, file) = file();
    let out = server.run(&file);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("maximum upload size"));
    server.finish();
}

#[test]
fn release_upload_keeps_possibly_published_files_after_confirmation_failure() {
    let storage = storage(200);
    let server = Server::new(|_| {
        let mut complete = api(
            "POST",
            "/api/v1/repos/alice/castle/releases/release-1/assets/asset-1/complete",
            json!({}),
            json!({}),
        );
        complete.status = 503;
        vec![lookup(512), prepare(&storage.host), complete]
    });
    let (_dir, file) = file();
    let out = server.run(&file);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Check the release before trying again."));
    server.finish();
    storage.finish();
}

#[test]
fn release_upload_cleans_up_failed_transfers_and_rejects_redirects() {
    for status in [500, 307] {
        let storage = storage(status);
        let server = Server::new(|_| {
            vec![
                lookup(512),
                prepare(&storage.host),
                api(
                    "DELETE",
                    "/api/v1/repos/alice/castle/releases/release-1/assets/asset-1",
                    Value::Null,
                    Value::Null,
                ),
            ]
        });
        let (_dir, file) = file();
        let out = server.run(&file);
        assert!(!out.status.success());
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(message.contains("storage upload failed"), "{message}");
        assert!(!message.contains("private-token"));
        assert!(!message.contains("private-secret"));
        server.finish();
        storage.finish();
    }
}
