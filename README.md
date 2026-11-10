# sculpt

The command line for [Sculptary](https://sculptary.com): version control and collaboration for 3D.

`sculpt` uses the Git you already have. It adds Sculptary sign-in, Git LFS defaults for model formats,
local model validation, geometry-aware diffs, and publishing.

```bash
sculpt login                      # paste a token from sculptary.com/settings/tokens
sculpt init                       # git init + LFS tracking for model formats
sculpt validate                   # check every model file in the repository
sculpt diff                       # git diff --stat plus triangle/vertex/mesh deltas
sculpt publish --private          # create the Sculptary repository and push
sculpt release v1.0.0 --notes "First release"
sculpt release upload v1.0.0 tooling.zip
sculpt clone owner/repository
sculpt preview models/castle.glb  # open the file on Sculptary
sculpt collection list northwind # user or organization
sculpt collection create "Castle kits" --owner northwind --slug castle-kits
sculpt collection add northwind/castle-kits northwind/medieval-castle --note "Use the tower"
sculpt pr comments northwind/medieval-castle 7
sculpt pr comment northwind/medieval-castle 7 --path models/castle.glb --at 1,2,3 --normal 0,1,0 --body "Round this edge"
sculpt pr reply northwind/medieval-castle 7 <comment-id> --body "Fixed"
sculpt pr resolve northwind/medieval-castle <comment-id>
sculpt pr unresolve northwind/medieval-castle <comment-id>
sculpt compare northwind/medieval-castle v1.0.0..main --path models/castle.glb --deviation
```

`status`, `add`, `commit`, `push` and `pull` pass through to Git; `add` warns when a model file is not
stored with LFS. `sculpt login` registers `sculpt` as a Git credential helper for the host, so plain
`git push` works too (skip with `--no-git-setup`).

Local inspection reads GLB, glTF, OBJ, STL and PLY. Other formats are versioned normally and processed by
Sculptary after a push.

## Collections and model review

Version 0.2 adds these remote commands. `--host` selects a server; saved credentials from `sculpt login`
are used when available, and public data can be read without signing in. Each command accepts `--json`
for pretty-printed API data (successful deletions print `null`). Human output uses sentences and tables.

| Command | Arguments and options |
| --- | --- |
| `sculpt collection list [owner]` | User or organization; defaults to the signed-in user. `--repo owner/repo` lists containing collections instead. |
| `sculpt collection show owner/slug` | Show collection details, repositories, notes and positions. |
| `sculpt collection create <name>` | `--owner`, `--slug`, `--description`, `--private` (public by default). |
| `sculpt collection edit owner/slug` | At least one of `--name`, `--slug`, `--description`, `--visibility public\|private`. |
| `sculpt collection delete owner/slug` | Delete the collection; repositories are retained. |
| `sculpt collection add owner/slug owner/repo` | `--note <text>` sets a note; omitting it preserves an existing note. |
| `sculpt collection remove owner/slug owner/repo` | Remove a repository from the collection. |
| `sculpt collection reorder owner/slug [owner/repo ...]` | Every visible repository exactly once, in the desired order. |
| `sculpt pr comments <repo> <number>` | List comments with asset path, position, normal and camera when present. |
| `sculpt pr comment <repo> <number>` | Required: `--path <asset> --at x,y,z --body <text>`. Optional: `--normal x,y,z`. |
| `sculpt pr reply <repo> <number> <comment-id>` | Required: `--body <text>`. Creates a real reply to the root comment. |
| `sculpt pr resolve <repo> <comment-id>` | Resolve as the comment author, PR author or a repository writer. |
| `sculpt pr unresolve <repo> <comment-id>` | Reopen a resolved thread with the same permissions. |
| `sculpt compare <repo> <base>..<head>` | Required: `--path <asset>`. `--deviation` includes surface measurements. |

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

## Release files

Version 0.4 adds `sculpt release upload <tag> <file>`. The release must already
exist. The repository defaults to the current Git origin; use `--repo owner/name`
to upload from another directory. `--host` selects the server and `--json` prints
the verified file metadata.

The command checks the release's upload limit, calculates SHA-256 with a fixed
buffer, streams the file directly to storage and confirms server verification.
Storage requests carry only the signed upload headers, without your API token,
and reject redirects. Failed transfers remove their pending upload when possible;
abandoned uploads expire automatically. If the verification response is lost,
check the release before retrying, since the file may already be published.

Existing `sculpt release <tag> --notes <text>` creation commands still work. To
create a release whose tag is literally `upload`, use `sculpt release -- upload`.

## Install

```bash
cargo install --git https://github.com/Sculptary/cli
```

Prebuilt binaries for macOS, Linux and Windows are attached to each
[release](https://github.com/Sculptary/cli/releases).

The release workflow also produces `sculpt.rb` for Homebrew, `sculpt.json` for
Scoop, `install.sh`, `install.ps1` and `SHA256SUMS`. These accompany the four
archives for Linux x86_64, macOS Apple silicon, macOS Intel and Windows x86_64.
Use a release that includes these files. There is no published Homebrew tap or
Scoop bucket; download the manifest or installer from that release's assets.

On Linux or macOS, run the downloaded installer:

```sh
sh install.sh
# Or choose a directory:
SCULPT_INSTALL_DIR="$HOME/bin" sh install.sh
```

The default directory is `$HOME/.local/bin`. The script checks the archive's
embedded SHA-256 before extracting and replacing `sculpt`. It stages the new
binary beside the destination and cleans up temporary files after failures.
It requires `curl`, `tar`, `mktemp` and `sha256sum` or `shasum`.

On Windows, run the downloaded PowerShell installer:

```powershell
.\install.ps1
# Or choose a directory:
.\install.ps1 -InstallDir "$HOME\bin"
```

The default directory is `%LOCALAPPDATA%\Sculptary\bin`. The installer requires
Windows x86_64, PowerShell 5.1 or later and the Windows `tar` command. It verifies
SHA-256 before replacing `sculpt.exe`. Add the chosen directory to your PATH;
neither installer changes PATH or requests administrator privileges.

To use the downloaded Scoop manifest, run `scoop install .\sculpt.json`.
To use the downloaded Homebrew formula in a local tap:

```sh
brew tap-new local/sculpt
cp sculpt.rb "$(brew --repository local/sculpt)/Formula/sculpt.rb"
brew install local/sculpt/sculpt
```

Both manifests pin archive checksums. `SHA256SUMS` covers archives and installer
files; `sha256sum -c SHA256SUMS` verifies a complete downloaded bundle. See
[release packaging](docs/releasing.md) for checks and a workflow preview that
creates build artifacts without publishing a release.

Version 0.5 renames the executable from `shape` to `sculpt`. Existing saved
credentials and `SCULPTARY_*` settings carry over. Run `sculpt login` once after
upgrading to update Git's credential helper, then use `sculpt` in scripts and
shell shortcuts. Release archives now start with `sculpt-`.

## Configuration

Credentials are stored in your platform config directory (`sculptary/config.toml`, owner-only on
Unix). Override with `SCULPTARY_CONFIG`; choose a host with `--host` or `SCULPTARY_HOST`.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
