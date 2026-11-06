# shape

The command line for [Sculptary](https://sculptary.com): version control and collaboration for 3D.

`shape` uses the Git you already have. It adds Sculptary sign-in, Git LFS defaults for model formats,
local model validation, geometry-aware diffs, and publishing.

```bash
shape login                      # paste a token from sculptary.com/settings/tokens
shape init                       # git init + LFS tracking for model formats
shape validate                   # check every model file in the repository
shape diff                       # git diff --stat plus triangle/vertex/mesh deltas
shape publish --private          # create the Sculptary repository and push
shape release v1.0.0 --notes "First release"
shape clone owner/repository
shape preview models/castle.glb  # open the file on Sculptary
```

`status`, `add`, `commit`, `push` and `pull` pass through to Git; `add` warns when a model file is not
stored with LFS. `shape login` registers `shape` as a Git credential helper for the host, so plain
`git push` works too (skip with `--no-git-setup`).

Local inspection reads GLB, glTF, OBJ, STL and PLY. Other formats are versioned normally and processed by
Sculptary after a push.

## Install

```bash
cargo install --git https://github.com/Sculptary/cli
```

Prebuilt binaries for macOS, Linux and Windows are attached to each
[release](https://github.com/Sculptary/cli/releases).

## Configuration

Credentials are stored in your platform config directory (`sculptary/config.toml`, owner-only on
Unix). Override with `SCULPTARY_CONFIG`; choose a host with `--host` or `SCULPTARY_HOST`.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
