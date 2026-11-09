//! Minimal client for the public Sculptary API (`/api/v1`).

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

pub struct Client {
    base: String,
    token: Option<String>,
    http: reqwest::blocking::Client,
}

#[derive(Debug, Deserialize)]
pub struct Repo {
    pub full_name: String,
    pub clone_http: String,
    pub visibility: String,
}

#[derive(Debug, Deserialize)]
pub struct Me {
    pub username: String,
}

impl Client {
    pub fn new(host: &str, token: Option<String>) -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("sculpt-cli/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("http client");
        Self {
            base: format!("{}/api/v1", host.trim_end_matches('/')),
            token,
            http,
        }
    }

    fn send<T: DeserializeOwned>(&self, req: reqwest::blocking::RequestBuilder) -> Result<T> {
        let req = match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        };
        let resp = req.send()?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .map(String::from)
                })
                .unwrap_or_else(|| format!("request failed with status {status}"));
            return Err(anyhow!(msg));
        }
        Ok(serde_json::from_str(if text.is_empty() {
            "null"
        } else {
            &text
        })?)
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.send(self.http.get(format!("{}{path}", self.base)))
    }

    pub fn get_query<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        self.send(self.http.get(format!("{}{path}", self.base)).query(query))
    }

    pub fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        self.send(self.http.post(format!("{}{path}", self.base)).json(body))
    }

    pub fn patch<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        self.send(self.http.patch(format!("{}{path}", self.base)).json(body))
    }

    pub fn put<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        self.send(self.http.put(format!("{}{path}", self.base)).json(body))
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        self.send(self.http.delete(format!("{}{path}", self.base)))
    }

    pub fn me(&self) -> Result<Me> {
        self.get("/user")
    }
}
