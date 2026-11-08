//! Remote collection, spatial review and model comparison commands.

use anyhow::{bail, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::api::Client;

#[derive(Clone, Debug, ValueEnum)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    fn as_str(&self) -> &str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

#[derive(Subcommand)]
pub enum CollectionCmd {
    /// List a user's or organization's collections (default: signed-in user).
    List {
        #[arg(conflicts_with = "repo")]
        owner: Option<String>,
        /// List collections containing this owner/repository instead.
        #[arg(long, value_parser = full_name)]
        repo: Option<String>,
    },
    /// Show owner/slug, including repositories and notes.
    Show {
        #[arg(value_parser = full_name)]
        collection: String,
    },
    /// Create a collection, optionally under an organization.
    Create {
        name: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        slug: Option<String>,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        private: bool,
    },
    /// Update supplied fields of owner/slug.
    #[command(group(clap::ArgGroup::new("changes").args(["name", "slug", "description", "visibility"]).required(true).multiple(true)))]
    Edit {
        #[arg(value_parser = full_name)]
        collection: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        slug: Option<String>,
        /// Use an empty string to clear the description.
        #[arg(long)]
        description: Option<String>,
        #[arg(long, value_enum)]
        visibility: Option<Visibility>,
    },
    /// Delete a collection (repositories are retained).
    Delete {
        #[arg(value_parser = full_name)]
        collection: String,
    },
    /// Add owner/repository, or update its note.
    Add {
        #[arg(value_parser = full_name)]
        collection: String,
        #[arg(value_parser = full_name)]
        repository: String,
        /// Omit to preserve an existing note; use an empty string to clear it.
        #[arg(long)]
        note: Option<String>,
    },
    /// Remove owner/repository from a collection.
    Remove {
        #[arg(value_parser = full_name)]
        collection: String,
        #[arg(value_parser = full_name)]
        repository: String,
    },
    /// Supply every visible repository exactly once in the desired order.
    Reorder {
        #[arg(value_parser = full_name)]
        collection: String,
        #[arg(value_parser = full_name)]
        repositories: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum PrCmd {
    /// List PR comments, including model anchors.
    Comments {
        #[arg(value_parser = full_name)]
        repo: String,
        #[arg(value_parser = clap::value_parser!(u64).range(1..))]
        number: u64,
    },
    /// Reply to a root comment in a pull request.
    Reply {
        #[arg(value_parser = full_name)]
        repo: String,
        #[arg(value_parser = clap::value_parser!(u64).range(1..))]
        number: u64,
        #[arg(value_parser = comment_id)]
        comment_id: String,
        #[arg(long)]
        body: String,
    },
    /// Resolve a comment thread.
    Resolve {
        #[arg(value_parser = full_name)]
        repo: String,
        #[arg(value_parser = comment_id)]
        comment_id: String,
    },
    /// Reopen a resolved comment thread.
    Unresolve {
        #[arg(value_parser = full_name)]
        repo: String,
        #[arg(value_parser = comment_id)]
        comment_id: String,
    },
    /// Pin a comment to a 3D model point.
    Comment {
        #[arg(value_parser = full_name)]
        repo: String,
        #[arg(value_parser = clap::value_parser!(u64).range(1..))]
        number: u64,
        #[arg(long)]
        path: String,
        /// Position in model coordinates, as x,y,z.
        #[arg(long, value_parser = vector3, allow_hyphen_values = true)]
        at: [f64; 3],
        #[arg(long, value_parser = vector3, allow_hyphen_values = true)]
        normal: Option<[f64; 3]>,
        #[arg(long)]
        body: String,
    },
}

#[derive(Args)]
pub struct CompareArgs {
    #[arg(value_parser = full_name)]
    repo: String,
    /// Two refs separated by two dots: base..head.
    #[arg(value_parser = revision_range)]
    range: (String, String),
    #[arg(long)]
    path: String,
    /// Include surface deviation, accuracy and the largest change.
    #[arg(long)]
    deviation: bool,
}

pub struct Output {
    data: Value,
    human: String,
}

impl Output {
    pub fn print(self, json: bool) -> Result<()> {
        if json {
            println!("{}", serde_json::to_string_pretty(&self.data)?);
        } else {
            println!("{}", self.human);
        }
        Ok(())
    }
}

fn comment_id(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("expected a comment ID".into());
    }
    Ok(value.into())
}

fn full_name(value: &str) -> Result<String, String> {
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() != 2
        || parts
            .iter()
            .any(|s| s.is_empty() || *s == "." || *s == "..")
        || value
            .chars()
            .any(|c| c.is_whitespace() || "?#\\%".contains(c))
    {
        return Err("expected owner/name".into());
    }
    Ok(value.to_string())
}

fn vector3(value: &str) -> Result<[f64; 3], String> {
    let values: Vec<f64> = value
        .split(',')
        .map(|v| {
            v.trim()
                .parse::<f64>()
                .map_err(|_| "expected x,y,z numbers".to_string())
        })
        .collect::<Result<_, _>>()?;
    if values.len() != 3 || values.iter().any(|v| !v.is_finite()) {
        return Err("expected three finite numbers as x,y,z".into());
    }
    Ok([values[0], values[1], values[2]])
}

fn revision_range(value: &str) -> Result<(String, String), String> {
    match value.split_once("..") {
        Some((base, head))
            if !base.is_empty()
                && !head.is_empty()
                && !head.starts_with('.')
                && !head.contains("..") =>
        {
            Ok((base.into(), head.into()))
        }
        _ => Err("expected base..head with two nonempty refs".into()),
    }
}

pub fn collection(api: &Client, cmd: CollectionCmd) -> Result<Output> {
    let data: Value = match cmd {
        CollectionCmd::List { owner, repo } => {
            let path = if let Some(repo) = repo {
                format!("/repos/{repo}/collections")
            } else {
                let owner = match owner {
                    Some(owner) => owner,
                    None => api.me()?.username,
                };
                format!("/users/{owner}/collections")
            };
            let data: Value = api.get(&path)?;
            return Ok(Output {
                human: collection_list(&data),
                data,
            });
        }
        CollectionCmd::Show { collection } => api.get(&format!("/collections/{collection}"))?,
        CollectionCmd::Create {
            name,
            owner,
            slug,
            description,
            private,
        } => api.post(
            "/collections",
            &json!({"name": name, "owner": owner, "slug": slug, "description": description,
                    "visibility": if private { "private" } else { "public" }}),
        )?,
        CollectionCmd::Edit {
            collection,
            name,
            slug,
            description,
            visibility,
        } => {
            let mut body = json!({});
            for (key, value) in [
                ("name", name),
                ("slug", slug),
                ("description", description),
                ("visibility", visibility.map(|v| v.as_str().to_string())),
            ] {
                if let Some(value) = value {
                    body[key] = json!(value);
                }
            }
            api.patch(&format!("/collections/{collection}"), &body)?
        }
        CollectionCmd::Delete { collection } => {
            api.delete(&format!("/collections/{collection}"))?;
            return Ok(Output {
                data: Value::Null,
                human: format!("Deleted collection {collection}."),
            });
        }
        CollectionCmd::Add {
            collection,
            repository,
            note,
        } => api.put(
            &format!("/collections/{collection}/items/{repository}"),
            &json!({"note": note}),
        )?,
        CollectionCmd::Remove {
            collection,
            repository,
        } => {
            api.delete(&format!("/collections/{collection}/items/{repository}"))?;
            return Ok(Output {
                data: Value::Null,
                human: format!("Removed {repository} from {collection}."),
            });
        }
        CollectionCmd::Reorder {
            collection,
            repositories,
        } => api.post(
            &format!("/collections/{collection}/order"),
            &json!({"full_names": repositories}),
        )?,
    };
    Ok(Output {
        human: collection_detail(&data),
        data,
    })
}

pub fn pr(api: &Client, cmd: PrCmd) -> Result<Output> {
    let data: Value = match cmd {
        PrCmd::Comments { repo, number } => {
            let detail: Value = api.get(&format!("/repos/{repo}/pulls/{number}"))?;
            let data = detail["comments"].clone();
            let human = match data.as_array().filter(|a| !a.is_empty()) {
                Some(comments) => comments
                    .iter()
                    .map(comment_text)
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                None => "No comments found.".into(),
            };
            return Ok(Output { data, human });
        }
        PrCmd::Reply {
            repo,
            number,
            comment_id,
            body,
        } => {
            if body.trim().is_empty() {
                bail!("body must not be empty");
            }
            api.post(
                &format!("/repos/{repo}/pulls/{number}/comments"),
                &json!({"body": body, "parent_id": comment_id}),
            )?
        }
        PrCmd::Resolve { repo, comment_id } => api.post(
            &format!("/repos/{repo}/comments/{comment_id}/resolve"),
            &json!({}),
        )?,
        PrCmd::Unresolve { repo, comment_id } => api.post(
            &format!("/repos/{repo}/comments/{comment_id}/unresolve"),
            &json!({}),
        )?,
        PrCmd::Comment {
            repo,
            number,
            path,
            at,
            normal,
            body,
        } => {
            if path.trim().is_empty() || body.trim().is_empty() {
                bail!("path and body must not be empty");
            }
            api.post(&format!("/repos/{repo}/pulls/{number}/comments"),
                     &json!({"path": path, "body": body, "spatial_anchor": {"point": at, "normal": normal}}))?
        }
    };
    Ok(Output {
        human: comment_text(&data),
        data,
    })
}

pub fn compare(api: &Client, args: CompareArgs) -> Result<Output> {
    if args.path.trim().is_empty() {
        bail!("path must not be empty");
    }
    let query = [
        ("base", args.range.0.as_str()),
        ("head", args.range.1.as_str()),
        ("path", args.path.as_str()),
    ];
    let mut data: Value = api.get_query(&format!("/repos/{}/asset-compare", args.repo), &query)?;
    let mut human = metadata_text(&args.path, &data);
    if args.deviation {
        let deviation: Value =
            api.get_query(&format!("/repos/{}/geometry-compare", args.repo), &query)?;
        human.push_str(&format!("\n{}", deviation_text(&deviation)));
        data["deviation"] = deviation;
    }
    Ok(Output { data, human })
}

fn text<'a>(data: &'a Value, key: &str) -> &'a str {
    data[key].as_str().unwrap_or("unknown")
}

fn collection_list(data: &Value) -> String {
    match data.as_array().filter(|a| !a.is_empty()) {
        Some(items) => {
            let mut rows = vec!["COLLECTION\tNAME\tVISIBILITY\tITEMS".into()];
            rows.extend(items.iter().map(|c| {
                format!(
                    "{}/{}\t{}\t{}\t{}",
                    text(&c["owner"], "slug"),
                    text(c, "slug"),
                    text(c, "name"),
                    text(c, "visibility"),
                    c["item_count"]
                )
            }));
            rows.join("\n")
        }
        None => "No collections found.".into(),
    }
}

fn collection_detail(data: &Value) -> String {
    let mut rows = vec![format!(
        "{}/{}: {} ({}; {} repositories).",
        text(&data["owner"], "slug"),
        text(data, "slug"),
        text(data, "name"),
        text(data, "visibility"),
        data["item_count"]
    )];
    if let Some(description) = data["description"].as_str().filter(|s| !s.is_empty()) {
        rows.push(description.into());
    }
    if let Some(items) = data["items"].as_array().filter(|a| !a.is_empty()) {
        rows.push("POSITION\tREPOSITORY\tNOTE".into());
        rows.extend(items.iter().map(|item| {
            format!(
                "{}\t{}\t{}",
                item["position"],
                text(&item["repository"], "full_name"),
                item["note"].as_str().unwrap_or("")
            )
        }));
    }
    rows.join("\n")
}

fn comment_text(data: &Value) -> String {
    let mut rows = vec![format!(
        "{} by {}: {}",
        text(data, "id"),
        data["author"]["username"]
            .as_str()
            .unwrap_or("deleted user"),
        text(data, "body")
    )];
    if let Some(path) = data["path"].as_str() {
        rows.push(format!("Asset: {path}."));
    }
    if let Some(anchor) = data["spatial_anchor"].as_object() {
        for (key, label) in [
            ("point", "Position"),
            ("normal", "Normal"),
            ("camera", "Camera"),
            ("object", "Object"),
            ("mesh", "Mesh"),
        ] {
            if let Some(value) = anchor.get(key).filter(|v| !v.is_null()) {
                rows.push(format!("{label}: {value}."));
            }
        }
    }
    if let Some(at) = data["resolved_at"].as_str() {
        rows.push(format!(
            "Resolved by {} at {at}.",
            data["resolved_by"]["username"]
                .as_str()
                .unwrap_or("deleted user")
        ));
    }
    if let Some(replies) = data["replies"].as_array() {
        for reply in replies {
            rows.push(format!(
                "Reply:\n  {}",
                comment_text(reply).replace('\n', "\n  ")
            ));
        }
    }
    rows.join("\n")
}

fn metadata_text(path: &str, data: &Value) -> String {
    let mut rows = vec![format!("{path}: {}.", text(data, "status"))];
    for side in ["base", "head"] {
        if let Some(status) = data[side]["status"].as_str().filter(|s| *s != "ready") {
            rows.push(format!("The {side} asset is {status}."));
        }
    }
    if let Some(stats) = data["diff"]["statistics"]
        .as_object()
        .filter(|s| !s.is_empty())
    {
        rows.push("STATISTIC\tBASE\tHEAD\tDELTA".into());
        rows.extend(stats.iter().map(|(key, value)| {
            format!(
                "{key}\t{}\t{}\t{}",
                number(&value["base"]),
                number(&value["head"]),
                number(&value["delta"])
            )
        }));
    }
    if let Some(changed) = data["diff"]["bounding_box"]["size_changed"].as_bool() {
        rows.push(format!(
            "Bounding box size {}.",
            if changed { "changed" } else { "is unchanged" }
        ));
    }
    for key in ["added", "removed"] {
        if let Some(materials) = data["diff"]["materials"][key]
            .as_array()
            .filter(|m| !m.is_empty())
        {
            rows.push(format!(
                "Materials {key}: {}.",
                materials
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    rows.join("\n")
}

fn number(value: &Value) -> String {
    if value.is_number() {
        value.to_string()
    } else {
        "unknown".into()
    }
}

fn deviation_text(data: &Value) -> String {
    let mut rows = vec![format!("Surface deviation is {}.", text(data, "status"))];
    let result = &data["result"];
    if result.get("method").is_some() {
        rows.push(format!("Method: {}. Confidence: {}. Resolution: {} model units. Largest change (Hausdorff): {} model units.",
            text(result, "method"), text(result, "confidence"), number(&result["resolution"]), number(&result["hausdorff"])));
    }
    for key in ["reason", "error"] {
        if let Some(message) = result[key].as_str() {
            rows.push(message.into());
        }
    }
    if let Some(notes) = result["notes"].as_array() {
        rows.extend(notes.iter().filter_map(Value::as_str).map(String::from));
    }
    rows.join("\n")
}
