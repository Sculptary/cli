# Release packaging

The release workflow builds four native targets: Linux x86_64 GNU, macOS Apple
silicon, macOS Intel and Windows x86_64 MSVC. Linux builds use Ubuntu 24.04.
Each archive contains `sculpt` or `sculpt.exe` and both licence texts. Archive
timestamps, ownership and permissions are normalized so packing the same input
again produces the same bytes.

The tag must be a semantic version prefixed with `v`, matching `Cargo.toml`.
Invalid tags fail before any build starts. `scripts/release.py` reads and hashes
files with bounded buffers and rejects missing targets, mixed releases,
unexpected archive members, links and incomplete checksum manifests.

## Check a change

```sh
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python -m unittest discover -s tests -p 'test_*.py' -v
```

Python 3.11 or later is required for the packaging tools. They use only the
standard library and do not add a runtime dependency to `sculpt`.

CI also sets `SCULPT_TEST_BINARY` to the compiled command and runs the generated
installer against local archives. POSIX tests exercise all supported Unix target
selections, checksum and download failures, replacement of an existing command,
paths with spaces and brackets, cleanup and unsupported architectures. Windows
tests execute the generated PowerShell script and the installed native binary.
The macOS job installs and tests the formula with Homebrew through an isolated
local fixture tap. These checks do not publish a package or a tap.

## Preview the complete bundle

Run the Release workflow manually on `main`:

```sh
gh workflow run release.yml --ref main
```

After it succeeds, download the `sculpt-release-bundle` workflow artifact from
that run. It contains all four archives and these generated assets:

- `sculpt.rb`: Homebrew formula for macOS Intel, Apple silicon and Linux x86_64.
- `sculpt.json`: Scoop manifest for Windows x86_64.
- `install.sh`: POSIX installer with a checksum for each supported Unix target.
- `install.ps1`: PowerShell installer with the Windows archive checksum.
- `SHA256SUMS`: SHA-256 hashes of every archive and generated installer file.

Verify the downloaded directory with:

```sh
python scripts/release.py verify path/to/bundle
```

Manual runs use the current Cargo version and create workflow artifacts only.
Their generated download URLs describe a future tagged release; the preview
does not create that tag or make those URLs public. Each platform's build is
also available as a separate workflow artifact.

## Tagged releases

A tag push runs the same validation, build and assembly. A single final step
attaches the complete bundle to the GitHub release after every target succeeds.
Nothing is attached if a target fails. The generator checks actual archive
contents and derives all hashes from those files rather than accepting supplied
checksum values. The bundle can also be recreated locally from four archives:

```sh
python scripts/release.py generate --tag v0.5.0 --artifacts release --output release
python scripts/release.py verify release
```

Use the actual matching tag when the Cargo version changes. The workflow does
not update a Homebrew tap, Scoop bucket, crates.io or any other package registry.
Publishing and registry accounts remain separate owner tasks.
