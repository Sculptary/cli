//! Exercise the executable against a local HTTP server; no Sculptary network access.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Expected {
    method: &'static str,
    target: String,
    body: Option<Value>,
    status: u16,
    response: Value,
}

fn request(method: &'static str, target: &str, body: Option<Value>, response: Value) -> Expected {
    Expected {
        method,
        target: target.into(),
        body,
        status: 200,
        response,
    }
}

struct Server {
    host: String,
    config: tempfile::TempDir,
    worker: JoinHandle<()>,
}

impl Server {
    fn new(expected: Vec<Expected>, authenticated: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let config = tempfile::tempdir().unwrap();
        if authenticated {
            std::fs::write(
                config.path().join("config.toml"),
                format!("[hosts.\"{host}\"]\nusername = \"alice\"\ntoken = \"test-token\"\n"),
            )
            .unwrap();
        }
        let worker = thread::spawn(move || {
            for expected in expected {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        // Accepted sockets inherit non-blocking mode from the listener on
                        // Windows; reads below must block.
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false).unwrap();
                            break stream;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "no request for {}",
                                expected.target
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("accept: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let (headers, body) = read_request(&mut stream);
                assert_eq!(
                    headers.lines().next().unwrap(),
                    format!("{} {} HTTP/1.1", expected.method, expected.target)
                );
                let headers = headers.to_ascii_lowercase();
                assert!(headers.contains(concat!(
                    "user-agent: sculpt-cli/",
                    env!("CARGO_PKG_VERSION")
                )));
                if authenticated {
                    assert!(headers.contains("authorization: bearer test-token"));
                } else {
                    assert!(!headers.contains("authorization:"));
                }
                assert_eq!(
                    if body.is_empty() {
                        None
                    } else {
                        Some(serde_json::from_slice::<Value>(&body).unwrap())
                    },
                    expected.body
                );
                let body = if expected.status == 204 {
                    String::new()
                } else {
                    expected.response.to_string()
                };
                write!(stream, "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    expected.status, body.len(), body).unwrap();
            }
        });
        Self {
            host,
            config,
            worker,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sculpt"))
            .args(["--host", &self.host])
            .args(args)
            .env("SCULPTARY_CONFIG", self.config.path().join("config.toml"))
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
        assert_ne!(n, 0, "request ended before headers");
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
        assert_ne!(n, 0, "request ended before body");
        data.extend_from_slice(&buf[..n]);
    }
    (headers, data[end..end + length].to_vec())
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn collection() -> Value {
    json!({"id":"c1", "owner":{"slug":"studio", "kind":"organization"}, "slug":"kits", "name":"Kits",
        "description":"Reusable models", "visibility":"private", "item_count":1,
        "items":[{"repository":{"full_name":"alice/castle"},"note":"Use the tower","position":0}]})
}

fn comment() -> Value {
    json!({"id":"comment-1", "author":{"username":"alice"}, "body":"Fix the tower", "path":"models/castle.glb",
        "spatial_anchor":{"point":[1,-2,3], "normal":[0,1,0], "object":"Tower",
            "camera":{"position":[4,5,6],"target":[1,2,3],"zoom":1.5}}})
}

#[test]
fn reply_resolve_and_reopen_threads() {
    let server = Server::new(
        vec![
            request(
                "POST",
                "/api/v1/repos/alice/castle/pulls/7/comments",
                Some(json!({"body":"Raised", "parent_id":"comment-1"})),
                json!({"id":"reply-1", "body":"Raised", "parent_id":"comment-1", "author":{"username":"alice"}}),
            ),
            request(
                "POST",
                "/api/v1/repos/alice/castle/comments/comment-1/resolve",
                Some(json!({})),
                json!({"id":"comment-1", "body":"Fix", "resolved_at":"2026-11-08", "resolved_by":{"username":"alice"}}),
            ),
            request(
                "POST",
                "/api/v1/repos/alice/castle/comments/comment-1/unresolve",
                Some(json!({})),
                json!({"id":"comment-1", "body":"Fix", "resolved_at":null}),
            ),
        ],
        true,
    );
    assert!(stdout(server.run(&[
        "pr",
        "reply",
        "alice/castle",
        "7",
        "comment-1",
        "--body",
        "Raised"
    ]))
    .contains("Raised"));
    assert!(
        stdout(server.run(&["pr", "resolve", "alice/castle", "comment-1"])).contains("Resolved")
    );
    assert!(stdout(server.run(&["pr", "unresolve", "alice/castle", "comment-1"])).contains("Fix"));
    server.finish();
}

#[test]
fn threaded_payloads_and_permission_errors() {
    let c = json!({"id":"comment-1", "body":"Raise the roof", "author":{"username":"alice"},
                   "resolved_at":"2026-11-08", "resolved_by":{"username":"alice"},
                   "replies":[{"id":"reply-1", "parent_id":"comment-1", "body":"Raised", "author":{"username":"bob"}}]});
    let mut expected = vec![
        request(
            "GET",
            "/api/v1/repos/alice/castle/pulls/7",
            None,
            json!({"comments":[c]}),
        ),
        request(
            "GET",
            "/api/v1/repos/alice/castle/pulls/7",
            None,
            json!({"comments":[c]}),
        ),
    ];
    for (target, body) in [
        (
            "/api/v1/repos/alice/castle/comments/comment-1/resolve",
            json!({}),
        ),
        (
            "/api/v1/repos/alice/castle/comments/comment-1/unresolve",
            json!({}),
        ),
        (
            "/api/v1/repos/alice/castle/pulls/7/comments",
            json!({"body":"Raised", "parent_id":"comment-1"}),
        ),
    ] {
        expected.push(Expected {
            method: "POST",
            target: target.into(),
            body: Some(body),
            status: 403,
            response: json!({"error":{"code":"forbidden", "message":"Thread access denied."}}),
        });
    }
    let server = Server::new(expected, true);
    let out = stdout(server.run(&["pr", "comments", "alice/castle", "7"]));
    assert!(out.contains("Resolved by alice") && out.contains("Reply:") && out.contains("Raised"));
    let data: Value = serde_json::from_str(&stdout(server.run(&[
        "pr",
        "comments",
        "alice/castle",
        "7",
        "--json",
    ])))
    .unwrap();
    assert_eq!(data[0]["replies"][0]["parent_id"], "comment-1");
    for args in [
        vec!["pr", "resolve", "alice/castle", "comment-1"],
        vec!["pr", "unresolve", "alice/castle", "comment-1"],
        vec![
            "pr",
            "reply",
            "alice/castle",
            "7",
            "comment-1",
            "--body",
            "Raised",
        ],
    ] {
        let result = server.run(&args);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Thread access denied."));
    }
    server.finish();
}

#[test]
fn collection_crud_items_and_order_json() {
    let c = collection();
    let mut delete = request(
        "DELETE",
        "/api/v1/collections/studio/kits",
        None,
        Value::Null,
    );
    delete.status = 204;
    let mut remove = request(
        "DELETE",
        "/api/v1/collections/studio/kits/items/alice/castle",
        None,
        Value::Null,
    );
    remove.status = 204;
    let server = Server::new(
        vec![
            request("GET", "/api/v1/users/studio/collections", None, json!([c])),
            request("GET", "/api/v1/collections/studio/kits", None, c.clone()),
            request(
                "POST",
                "/api/v1/collections",
                Some(json!({"name":"Kits","owner":"studio","slug":"kits",
            "description":"Reusable models","visibility":"private"})),
                c.clone(),
            ),
            request(
                "PATCH",
                "/api/v1/collections/studio/kits",
                Some(json!({"name":"New kits", "slug":"new",
            "description":"", "visibility":"public"})),
                c.clone(),
            ),
            request(
                "PUT",
                "/api/v1/collections/studio/kits/items/alice/castle",
                Some(json!({"note":"Use the tower"})),
                c.clone(),
            ),
            request(
                "PUT",
                "/api/v1/collections/studio/kits/items/alice/castle",
                Some(json!({"note":null})),
                c.clone(),
            ),
            request(
                "PUT",
                "/api/v1/collections/studio/kits/items/alice/castle",
                Some(json!({"note":""})),
                c.clone(),
            ),
            request(
                "POST",
                "/api/v1/collections/studio/kits/order",
                Some(json!({"full_names":["alice/castle"]})),
                c.clone(),
            ),
            request(
                "GET",
                "/api/v1/repos/alice/castle/collections",
                None,
                json!([c]),
            ),
            remove,
            delete,
        ],
        true,
    );
    let commands: Vec<Vec<&str>> = vec![
        vec!["list", "studio"],
        vec!["show", "studio/kits"],
        vec![
            "create",
            "Kits",
            "--owner",
            "studio",
            "--slug",
            "kits",
            "--description",
            "Reusable models",
            "--private",
        ],
        vec![
            "edit",
            "studio/kits",
            "--name",
            "New kits",
            "--slug",
            "new",
            "--description",
            "",
            "--visibility",
            "public",
        ],
        vec![
            "add",
            "studio/kits",
            "alice/castle",
            "--note",
            "Use the tower",
        ],
        vec!["add", "studio/kits", "alice/castle"],
        vec!["add", "studio/kits", "alice/castle", "--note", ""],
        vec!["reorder", "studio/kits", "alice/castle"],
        vec!["list", "--repo", "alice/castle"],
        vec!["remove", "studio/kits", "alice/castle"],
        vec!["delete", "studio/kits"],
    ];
    for (i, command) in commands.iter().enumerate() {
        let args = [&["collection"][..], command.as_slice(), &["--json"]].concat();
        let result: Value = serde_json::from_str(&stdout(server.run(&args))).unwrap();
        assert_eq!(
            result,
            if i == 0 || i == 8 {
                json!([c])
            } else if i >= 9 {
                Value::Null
            } else {
                c.clone()
            }
        );
    }
    server.finish();
}

#[test]
fn collection_defaults_and_human_output() {
    let server = Server::new(
        vec![
            request("GET", "/api/v1/user", None, json!({"username":"alice"})),
            request(
                "GET",
                "/api/v1/users/alice/collections",
                None,
                json!([collection()]),
            ),
            request("GET", "/api/v1/collections/studio/kits", None, collection()),
            request(
                "POST",
                "/api/v1/collections",
                Some(
                    json!({"name":"Kits","owner":null,"slug":null,"description":"","visibility":"public"}),
                ),
                collection(),
            ),
            request(
                "POST",
                "/api/v1/collections/studio/kits/order",
                Some(json!({"full_names":[]})),
                collection(),
            ),
            request(
                "PATCH",
                "/api/v1/collections/studio/kits",
                Some(json!({"description":""})),
                collection(),
            ),
        ],
        true,
    );
    assert!(stdout(server.run(&["collection", "list"])).contains("studio/kits\tKits\tprivate\t1"));
    let output = stdout(server.run(&["collection", "show", "studio/kits"]));
    assert!(output.contains("Reusable models") && output.contains("alice/castle\tUse the tower"));
    assert!(stdout(server.run(&["collection", "create", "Kits"])).contains("studio/kits"));
    stdout(server.run(&["collection", "reorder", "studio/kits"]));
    stdout(server.run(&["collection", "edit", "studio/kits", "--description", ""]));
    server.finish();
}

#[test]
fn public_reads_and_empty_lists() {
    let server = Server::new(
        vec![
            request("GET", "/api/v1/users/studio/collections", None, json!([])),
            request(
                "GET",
                "/api/v1/repos/alice/castle/pulls/7",
                None,
                json!({"comments":[]}),
            ),
        ],
        false,
    );
    assert_eq!(
        stdout(server.run(&["collection", "list", "studio"])).trim(),
        "No collections found."
    );
    assert_eq!(
        stdout(server.run(&["pr", "comments", "alice/castle", "7"])).trim(),
        "No comments found."
    );
    server.finish();
}

#[test]
fn spatial_comments_list_and_create() {
    let c = comment();
    let server = Server::new(
        vec![
            request(
                "GET",
                "/api/v1/repos/alice/castle/pulls/7",
                None,
                json!({"comments":[c]}),
            ),
            request(
                "GET",
                "/api/v1/repos/alice/castle/pulls/7",
                None,
                json!({"comments":[c]}),
            ),
            request(
                "POST",
                "/api/v1/repos/alice/castle/pulls/7/comments",
                Some(json!({"body":"Fix the tower", "path":"models/castle.glb",
            "spatial_anchor":{"point":[1.0,-2.0,3.0],"normal":[0.0,1.0,0.0]}})),
                c.clone(),
            ),
            request(
                "POST",
                "/api/v1/repos/alice/castle/pulls/7/comments",
                Some(json!({"body":"Fix the tower", "path":"models/castle.glb",
            "spatial_anchor":{"point":[-1.0,2.0,3.0],"normal":null}})),
                c.clone(),
            ),
        ],
        true,
    );
    let out = stdout(server.run(&["pr", "comments", "alice/castle", "7"]));
    for expected in [
        "Fix the tower",
        "Asset: models/castle.glb",
        "Position: [1,-2,3]",
        "Normal: [0,1,0]",
        "Camera:",
        "Tower",
    ] {
        assert!(out.contains(expected), "{out}");
    }
    assert_eq!(
        serde_json::from_str::<Value>(&stdout(server.run(&[
            "pr",
            "--json",
            "comments",
            "alice/castle",
            "7"
        ])))
        .unwrap(),
        json!([c])
    );
    let out = stdout(server.run(&[
        "pr",
        "comment",
        "alice/castle",
        "7",
        "--path",
        "models/castle.glb",
        "--at",
        "1,-2,3",
        "--normal",
        "0,1,0",
        "--body",
        "Fix the tower",
        "--json",
    ]));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), c);
    stdout(server.run(&[
        "pr",
        "comment",
        "alice/castle",
        "7",
        "--path",
        "models/castle.glb",
        "--at",
        "-1,2,3",
        "--body",
        "Fix the tower",
    ]));
    server.finish();
}

fn metadata() -> Value {
    json!({"status":"modified","base":{"status":"ready"},"head":{"status":"ready"}, "diff":{
        "statistics":{"triangle_count":{"base":10,"head":12,"delta":2}, "node_count":{"base":null,"head":2,"delta":null}},
        "bounding_box":{"base":null,"head":null,"size_changed":true}, "materials":{"added":["Stone"],"removed":["Wood"]}}})
}

#[test]
fn comparison_encodes_queries_and_keeps_heatmaps_in_json_only() {
    let target = "/api/v1/repos/alice/castle/asset-compare?base=feature%2Fa%2Bb&head=release%2F2&path=models%2Fnew+%231.glb";
    let geometry = target.replace("asset-compare", "geometry-compare");
    let deviation = json!({"id":"cmp-1","status":"ready","heatmap_url":"/api/v1/repos/a/b/comparisons/cmp-1/heatmap",
        "result":{"method":"sampled-surface-nearest-neighbour","confidence":"medium","resolution":0.02,"hausdorff":0.4,
            "heatmap":{"scale_min":0,"scale_max":0.4,"unit":"model units","values":[987654321]},"notes":["Distances are approximate."]}});
    let server = Server::new(
        vec![
            request("GET", target, None, metadata()),
            request("GET", &geometry, None, deviation.clone()),
            request("GET", target, None, metadata()),
            request("GET", &geometry, None, deviation.clone()),
            request("GET", target, None, metadata()),
        ],
        true,
    );
    let args = [
        "compare",
        "alice/castle",
        "feature/a+b..release/2",
        "--path",
        "models/new #1.glb",
    ];
    let out = stdout(server.run(&[&args[..], &["--deviation"]].concat()));
    for expected in [
        "triangle_count\t10\t12\t2",
        "node_count\tunknown\t2\tunknown",
        "Bounding box size changed",
        "Materials added: Stone",
        "Materials removed: Wood",
        "Method: sampled-surface-nearest-neighbour",
        "Confidence: medium",
        "Resolution: 0.02",
        "Largest change (Hausdorff): 0.4",
    ] {
        assert!(out.contains(expected), "{out}");
    }
    assert!(!out.contains("987654321") && !out.contains("heatmap"));
    let data: Value = serde_json::from_str(&stdout(
        server.run(&[&args[..], &["--deviation", "--json"]].concat()),
    ))
    .unwrap();
    assert_eq!(data["deviation"], deviation);
    let data: Value =
        serde_json::from_str(&stdout(server.run(&[&args[..], &["--json"]].concat()))).unwrap();
    assert_eq!(data, metadata());
    server.finish();
}

#[test]
fn comparison_handles_incomplete_and_noncomparable_results() {
    let target = "/api/v1/repos/a/b/asset-compare?base=main&head=feature&path=m.glb";
    let geometry = target.replace("asset-compare", "geometry-compare");
    let mut requests = Vec::new();
    for (status, result) in [
        ("waiting", json!({})),
        ("pending", json!({})),
        ("running", json!({})),
        ("failed", json!({"error":"Unsupported geometry"})),
        ("ready", json!({"comparable":false,"reason":"No surfaces"})),
    ] {
        requests.push(request(
            "GET",
            target,
            None,
            json!({"status":"added","base":null,"head":{"status":"pending"},"diff":null}),
        ));
        requests.push(request(
            "GET",
            &geometry,
            None,
            json!({"status":status,"id":null,"result":result,"heatmap_url":null}),
        ));
    }
    let server = Server::new(requests, false);
    for status in ["waiting", "pending", "running", "failed", "ready"] {
        let out = stdout(server.run(&[
            "compare",
            "a/b",
            "main..feature",
            "--path",
            "m.glb",
            "--deviation",
        ]));
        assert!(out.contains(&format!("Surface deviation is {status}.")));
        assert!(out.contains("The head asset is pending."));
        if status == "failed" {
            assert!(out.contains("Unsupported geometry"));
        }
        if status == "ready" {
            assert!(out.contains("No surfaces"));
        }
    }
    server.finish();
}

#[test]
fn api_errors_are_reported_with_failure_exit_codes() {
    let server = Server::new(
        vec![Expected {
            method: "DELETE",
            target: "/api/v1/collections/studio/kits".into(),
            body: None,
            status: 403,
            response: json!({"error":{"code":"forbidden","message":"You cannot manage this collection."}}),
        }],
        true,
    );
    let out = server.run(&["collection", "delete", "studio/kits", "--json"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("You cannot manage this collection."));
    server.finish();
}

#[test]
fn invalid_arguments_fail_before_http() {
    let server = Server::new(vec![], false);
    for args in [
        vec!["collection", "show", "bad"],
        vec!["collection", "edit", "a/b"],
        vec!["collection", "list", "alice", "--repo", "a/b"],
        vec!["pr", "comments", "a/b", "0"],
        vec!["pr", "reply", "a/b", "1", "root", "--body", " "],
        vec!["pr", "resolve", "a/b", "../root"],
        vec!["pr", "unresolve", "a/b", "root?query"],
        vec![
            "pr", "comment", "a/b", "1", "--path", "m.glb", "--at", "1,2", "--body", "Hi",
        ],
        vec![
            "pr", "comment", "a/b", "1", "--path", "m.glb", "--at", "NaN,2,3", "--body", "Hi",
        ],
        vec![
            "pr", "comment", "a/b", "1", "--path", "m.glb", "--at", "1,2,3", "--normal", "0,inf,1",
            "--body", "Hi",
        ],
        vec!["compare", "a/b", "main...head", "--path", "m.glb"],
        vec!["compare", "a/b", "..head", "--path", "m.glb"],
        vec!["compare", "a/b", "main..", "--path", "m.glb"],
        vec!["compare", "a/b", "main..head"],
    ] {
        assert!(!server.run(&args).status.success(), "accepted {args:?}");
    }
    server.finish();
}

#[test]
fn command_help_and_authentication_errors_use_sculpt() {
    let server = Server::new(vec![], false);
    let help = server.run(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("Usage: sculpt"));
    let version = server.run(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("sculpt "));
    let whoami = server.run(&["whoami"]);
    assert!(!whoami.status.success());
    assert!(String::from_utf8_lossy(&whoami.stderr).contains("run `sculpt login`"));
    server.finish();
}

#[test]
fn login_replaces_the_previous_git_helper_with_sculpt() {
    let server = Server::new(
        vec![request(
            "GET",
            "/api/v1/user",
            None,
            json!({"username":"alice"}),
        )],
        true,
    );
    let global = server.config.path().join("gitconfig");
    let key = format!("credential.{}.helper", server.host);
    assert!(Command::new("git")
        .args(["config", "--file"])
        .arg(&global)
        .args([&key, "!shape git-credential"])
        .status()
        .unwrap()
        .success());
    let output = Command::new(env!("CARGO_BIN_EXE_sculpt"))
        .args(["--host", &server.host, "login", "--token", "test-token"])
        .env("SCULPTARY_CONFIG", server.config.path().join("config.toml"))
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("SCULPTARY_HOST")
        .output()
        .unwrap();
    assert!(output.status.success());
    let helper = Command::new("git")
        .args(["config", "--file"])
        .arg(&global)
        .args(["--get-all", &key])
        .output()
        .unwrap();
    assert!(helper.status.success());
    assert_eq!(
        String::from_utf8_lossy(&helper.stdout).trim(),
        "!sculpt git-credential"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Configured Git to use sculpt"));
    server.finish();
}
