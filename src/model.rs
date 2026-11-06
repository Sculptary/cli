//! Lightweight, dependency-free model inspection for validation and diffs.
//!
//! These parsers read headers and counts; they do not build full scenes. The
//! Sculptary server performs complete processing after a push.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ModelInfo {
    pub format: String,
    pub size: u64,
    pub triangles: u64,
    pub vertices: u64,
    pub meshes: u64,
    pub materials: u64,
    pub textures: u64,
    pub animations: u64,
    pub bounds: Option<[[f32; 3]; 2]>,
    pub warnings: Vec<String>,
}

pub const EXTENSIONS: &[&str] = &[
    "glb", "gltf", "obj", "stl", "ply", "fbx", "blend", "3mf", "step", "stp", "usd", "usdz",
];

pub fn format_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

pub fn inspect(path: &Path) -> Result<ModelInfo> {
    let fmt = format_of(path).context("not a recognized model file")?;
    let size = std::fs::metadata(path)?.len();
    let mut info = match fmt.as_str() {
        "glb" => glb(path)?,
        "gltf" => gltf(path)?,
        "stl" => stl(path)?,
        "obj" => obj(path)?,
        "ply" => ply(path)?,
        other => ModelInfo {
            warnings: vec![format!(
                "{other} files are stored and versioned; local inspection is not available"
            )],
            ..Default::default()
        },
    };
    info.format = fmt;
    info.size = size;
    Ok(info)
}

fn extend(b: &mut Option<[[f32; 3]; 2]>, p: [f32; 3]) {
    match b {
        None => *b = Some([p, p]),
        Some([mn, mx]) => {
            for i in 0..3 {
                mn[i] = mn[i].min(p[i]);
                mx[i] = mx[i].max(p[i]);
            }
        }
    }
}

fn gltf_counts(json: &serde_json::Value, info: &mut ModelInfo) {
    let arr = |k: &str| {
        json.get(k)
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u64)
            .unwrap_or(0)
    };
    info.meshes = arr("meshes");
    info.materials = arr("materials");
    info.textures = arr("textures");
    info.animations = arr("animations");
    let accessors = json.get("accessors").and_then(|v| v.as_array());
    let mut tris = 0u64;
    let mut verts = 0u64;
    for mesh in json
        .get("meshes")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        for prim in mesh
            .get("primitives")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let count = |idx: Option<u64>| {
                idx.and_then(|i| accessors?.get(i as usize)?.get("count")?.as_u64())
                    .unwrap_or(0)
            };
            let pos = prim
                .get("attributes")
                .and_then(|a| a.get("POSITION"))
                .and_then(|v| v.as_u64());
            let v = count(pos);
            verts += v;
            let mode = prim.get("mode").and_then(|m| m.as_u64()).unwrap_or(4);
            if mode == 4 {
                let idx = prim.get("indices").and_then(|v| v.as_u64());
                tris += if idx.is_some() { count(idx) / 3 } else { v / 3 };
            }
            if let Some(acc) = pos.and_then(|i| accessors?.get(i as usize)) {
                let read = |k: &str| {
                    acc.get(k).and_then(|v| v.as_array()).map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_f64())
                            .map(|x| x as f32)
                            .collect::<Vec<_>>()
                    })
                };
                if let (Some(mn), Some(mx)) = (read("min"), read("max")) {
                    if mn.len() == 3 && mx.len() == 3 {
                        extend(&mut info.bounds, [mn[0], mn[1], mn[2]]);
                        extend(&mut info.bounds, [mx[0], mx[1], mx[2]]);
                    }
                }
            }
        }
    }
    info.triangles = tris;
    info.vertices = verts;
    for kind in ["buffers", "images"] {
        for item in json
            .get(kind)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(uri) = item.get("uri").and_then(|u| u.as_str()) {
                if !uri.starts_with("data:") {
                    info.warnings
                        .push(format!("references external file '{uri}'"));
                }
            }
        }
    }
}

fn glb(path: &Path) -> Result<ModelInfo> {
    let mut f = File::open(path)?;
    let mut header = [0u8; 20];
    f.read_exact(&mut header)
        .context("file too short for GLB")?;
    if &header[0..4] != b"glTF" {
        bail!("missing glTF magic");
    }
    let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    if version != 2 {
        bail!("unsupported glTF version {version}");
    }
    let declared = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
    let chunk_len = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    if &header[16..20] != b"JSON" {
        bail!("first chunk is not JSON");
    }
    if chunk_len > 64 * 1024 * 1024 {
        bail!("JSON chunk is implausibly large");
    }
    let mut buf = vec![0u8; chunk_len];
    f.read_exact(&mut buf).context("truncated JSON chunk")?;
    let json: serde_json::Value = serde_json::from_slice(&buf).context("invalid JSON chunk")?;
    let mut info = ModelInfo::default();
    let actual = std::fs::metadata(path)?.len();
    if declared != actual {
        info.warnings.push(format!(
            "header declares {declared} bytes but file has {actual}"
        ));
    }
    gltf_counts(&json, &mut info);
    Ok(info)
}

fn gltf(path: &Path) -> Result<ModelInfo> {
    let json: serde_json::Value =
        serde_json::from_reader(BufReader::new(File::open(path)?)).context("invalid glTF JSON")?;
    if json
        .get("asset")
        .and_then(|a| a.get("version"))
        .and_then(|v| v.as_str())
        != Some("2.0")
    {
        bail!("not a glTF 2.0 asset");
    }
    let mut info = ModelInfo::default();
    gltf_counts(&json, &mut info);
    Ok(info)
}

fn stl(path: &Path) -> Result<ModelInfo> {
    let size = std::fs::metadata(path)?.len();
    let mut f = File::open(path)?;
    let mut head = [0u8; 84];
    let n = f.read(&mut head)?;
    let mut info = ModelInfo {
        meshes: 1,
        ..Default::default()
    };
    if n == 84 {
        let tris = u32::from_le_bytes(head[80..84].try_into().unwrap()) as u64;
        if 84 + tris * 50 == size {
            info.triangles = tris;
            info.vertices = tris * 3;
            let mut rec = [0u8; 50];
            for _ in 0..tris.min(5_000_000) {
                f.read_exact(&mut rec)?;
                for v in 0..3 {
                    let o = 12 + v * 12;
                    let p = [0, 1, 2].map(|k| {
                        f32::from_le_bytes(rec[o + k * 4..o + k * 4 + 4].try_into().unwrap())
                    });
                    extend(&mut info.bounds, p);
                }
            }
            return Ok(info);
        }
    }
    // ASCII STL
    let reader = BufReader::new(File::open(path)?);
    let mut first = true;
    for line in reader.lines() {
        let line = line.context("STL is neither valid binary nor ASCII")?;
        let t = line.trim();
        if first {
            if !t.starts_with("solid") {
                bail!("STL is neither valid binary nor ASCII");
            }
            first = false;
        }
        if let Some(rest) = t.strip_prefix("vertex") {
            let p: Vec<f32> = rest
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            if p.len() == 3 {
                extend(&mut info.bounds, [p[0], p[1], p[2]]);
                info.vertices += 1;
            }
        } else if t.starts_with("facet") {
            info.triangles += 1;
        }
    }
    Ok(info)
}

fn obj(path: &Path) -> Result<ModelInfo> {
    let reader = BufReader::new(File::open(path)?);
    let mut info = ModelInfo::default();
    let mut groups = 0u64;
    let mut mtl_names = std::collections::HashSet::new();
    for line in reader.lines() {
        let line = line?;
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let p: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if p.len() == 3 {
                    info.vertices += 1;
                    extend(&mut info.bounds, [p[0], p[1], p[2]]);
                }
            }
            Some("f") => {
                let n = it.count() as u64;
                if n >= 3 {
                    info.triangles += n - 2;
                }
            }
            Some("o") | Some("g") => groups += 1,
            Some("usemtl") => {
                if let Some(name) = it.next() {
                    mtl_names.insert(name.to_string());
                }
            }
            Some("mtllib") => {
                for lib in it {
                    if !path.with_file_name(lib).exists() {
                        info.warnings.push(format!(
                            "material library '{lib}' not found next to the model"
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    info.meshes = groups.max(1);
    info.materials = mtl_names.len() as u64;
    Ok(info)
}

fn ply(path: &Path) -> Result<ModelInfo> {
    let reader = BufReader::new(File::open(path)?);
    let mut info = ModelInfo {
        meshes: 1,
        ..Default::default()
    };
    let mut lines = reader.lines();
    if lines.next().transpose()?.as_deref() != Some("ply") {
        bail!("missing PLY magic");
    }
    for line in lines {
        let line = line?;
        let mut it = line.split_whitespace();
        match (it.next(), it.next(), it.next()) {
            (Some("element"), Some("vertex"), Some(n)) => info.vertices = n.parse().unwrap_or(0),
            (Some("element"), Some("face"), Some(n)) => info.triangles = n.parse().unwrap_or(0),
            (Some("end_header"), _, _) => break,
            _ => {}
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn obj_counts_and_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.obj");
        std::fs::write(
            &p,
            "o tetra\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 2\nf 1 2 3\nf 1 2 4\nf 1 3 4\nf 2 3 4 1\n",
        )
        .unwrap();
        let i = inspect(&p).unwrap();
        assert_eq!((i.vertices, i.triangles, i.meshes), (4, 5, 1));
        assert_eq!(i.bounds.unwrap()[1], [1.0, 1.0, 2.0]);
    }

    #[test]
    fn binary_stl() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.stl");
        let mut f = File::create(&p).unwrap();
        f.write_all(&[0u8; 80]).unwrap();
        f.write_all(&1u32.to_le_bytes()).unwrap();
        let mut rec = vec![0u8; 12];
        for v in [[0f32, 0., 0.], [3., 0., 0.], [0., 4., 0.]] {
            for c in v {
                rec.extend(c.to_le_bytes());
            }
        }
        rec.extend([0u8, 0]);
        f.write_all(&rec).unwrap();
        drop(f);
        let i = inspect(&p).unwrap();
        assert_eq!(i.triangles, 1);
        assert_eq!(i.bounds.unwrap()[1], [3.0, 4.0, 0.0]);
    }

    #[test]
    fn glb_rejects_garbage_and_reads_json() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.glb");
        std::fs::write(&bad, b"nope").unwrap();
        assert!(inspect(&bad).is_err());
        let json = br#"{"asset":{"version":"2.0"},"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],"accessors":[{"count":3,"min":[0,0,0],"max":[1,1,1]},{"count":3}]}  "#;
        let mut glb = Vec::new();
        glb.extend(b"glTF");
        glb.extend(2u32.to_le_bytes());
        glb.extend(((20 + json.len()) as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(b"JSON");
        glb.extend(json);
        let good = dir.path().join("good.glb");
        std::fs::write(&good, glb).unwrap();
        let i = inspect(&good).unwrap();
        assert_eq!((i.meshes, i.triangles, i.vertices), (1, 1, 3));
        assert!(i.warnings.is_empty(), "{:?}", i.warnings);
    }
}
