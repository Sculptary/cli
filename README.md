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
shape collection list northwind # user or organization
shape collection create "Castle kits" --owner northwind --slug castle-kits
shape collection add northwind/castle-kits northwind/medieval-castle --note "Use the tower"
shape pr comments northwind/medieval-castle 7
shape pr comment northwind/medieval-castle 7 --path models/castle.glb --at 1,2,3 --normal 0,1,0 --body "Round this edge"
shape pr reply northwind/medieval-castle 7 <comment-id> --body "Fixed"
shape pr resolve northwind/medieval-castle <comment-id>
shape pr unresolve northwind/medieval-castle <comment-id>
shape compare northwind/medieval-castle v1.0.0..main --path models/castle.glb --deviation
```

`status`, `add`, `commit`, `push` and `pull` pass through to Git; `add` warns when a model file is not
stored with LFS. `shape login` registers `shape` as a Git credential helper for the host, so plain
`git push` works too (skip with `--no-git-setup`).

Local inspection reads GLB, glTF, OBJ, STL and PLY. Other formats are versioned normally and processed by
Sculptary after a push.

## Collections and model review

Version 0.2 adds these remote commands. `--host` selects a server; saved credentials from `shape login`
are used when available, and public data can be read without signing in. Each command accepts `--json`
for pretty-printed API data (successful deletions print `null`). Human output uses sentences and tables.

| Command | Arguments and options |
| --- | --- |
| `shape collection list [owner]` | User or organization; defaults to the signed-in user. `--repo owner/repo` lists containing collections instead. |
| `shape collection show owner/slug` | Show collection details, repositories, notes and positions. |
| `shape collection create <name>` | `--owner`, `--slug`, `--description`, `--private` (public by default). |
| `shape collection edit owner/slug` | At least one of `--name`, `--slug`, `--description`, `--visibility public\|private`. |
| `shape collection delete owner/slug` | Delete the collection; repositories are retained. |
| `shape collection add owner/slug owner/repo` | `--note <text>` sets a note; omitting it preserves an existing note. |
| `shape collection remove owner/slug owner/repo` | Remove a repository from the collection. |
| `shape collection reorder owner/slug [owner/repo ...]` | Every visible repository exactly once, in the desired order. |
| `shape pr comments <repo> <number>` | List comments with asset path, position, normal and camera when present. |
| `shape pr comment <repo> <number>` | Required: `--path <asset> --at x,y,z --body <text>`. Optional: `--normal x,y,z`. |
| `shape pr reply <repo> <number> <comment-id>` | Required: `--body <text>`. Creates a real reply to the root comment. |
| `shape pr resolve <repo> <comment-id>` | Resolve as the comment author, PR author or a repository writer. |
| `shape pr unresolve <repo> <comment-id>` | Reopen a resolved thread with the same permissions. |
| `shape compare <repo> <base>..<head>` | Required: `--path <asset>`. `--deviation` includes surface measurements. |

Use an empty string with `--description` or `--note` to clear it. Collection and comment lists are
unpaginated. Spatial coordinates must be three finite numbers in model coordinates; no normal is
assumed unless supplied. For example, `--at=-1,2,3` supports a negative coordinate.

Version 0.3 adds replies and resolution. Comment lists group one level of replies under each root and
include resolution details. JSON output retains `parent_id`, `replies`, `resolved_at` and `resolved_by`.

Comparison output includes count deltas, bounding box changes and added/removed materials. Surface
deviation includes method, confidence, resolution and largest change (Hausdorff distance, in model
units). Processing results report their current state; run the command again while waiting, pending
or running. Failed/non-comparable results show their error or reason. Heatmap data is included only
in JSON; with `--deviation`, the complete geometry response is under `deviation`, including the colour
scale and authenticated heatmap GLB URL. Human output never prints heatmap arrays.

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
