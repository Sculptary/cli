# shape

The command line for [ShapesDB](https://shapesdb.com): version control and collaboration for 3D.

`shape` uses the Git you already have. It adds ShapesDB sign-in, Git LFS defaults for model formats,
local model validation, geometry-aware diffs, and publishing.

```bash
shape login                      # paste a token from shapesdb.com/settings/tokens
shape init                       # git init + LFS tracking for model formats
shape validate                   # check every model file in the repository
shape diff                       # git diff --stat plus triangle/vertex/mesh deltas
shape publish --private          # create the ShapesDB repository and push
shape release v1.0.0 --notes "First release"
shape clone owner/repository
shape preview models/castle.glb  # open the file on ShapesDB
```

`status`, `add`, `commit`, `push` and `pull` pass through to Git; `add` warns when a model file is not
stored with LFS. `shape login` registers `shape` as a Git credential helper for the host, so plain
`git push` works too (skip with `--no-git-setup`).

Local inspection reads GLB, glTF, OBJ, STL and PLY. Other formats are versioned normally and processed by
ShapesDB after a push.

## Install

```bash
cargo install --git https://github.com/ShapesDB/cli
```

Prebuilt binaries for macOS, Linux and Windows are attached to each
[release](https://github.com/ShapesDB/cli/releases).

## Configuration

Credentials are stored in your platform config directory (`shapesdb/config.toml`, owner-only on
Unix). Override with `SHAPESDB_CONFIG`; choose a host with `--host` or `SHAPESDB_HOST`.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
