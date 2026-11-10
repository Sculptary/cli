"""Build release archives and checksum-pinned installer assets, without publishing."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-unknown-linux-gnu": "sculpt",
    "aarch64-apple-darwin": "sculpt",
    "x86_64-apple-darwin": "sculpt",
    "x86_64-pc-windows-msvc": "sculpt.exe",
}
METADATA = {"sculpt.rb", "sculpt.json", "install.sh", "install.ps1", "SHA256SUMS"}
LICENCES = {"LICENSE-MIT", "LICENSE-APACHE"}
DOWNLOADS = "https://github.com/Sculptary/cli/releases/download"


def version(tag):
    match = re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?", tag, flags=re.ASCII)
    if not match or any(part.isdigit() and len(part) > 1 and part[0] == "0" for part in (match[4] or "").split(".")):
        raise ValueError("Release tag must be v followed by a valid semantic version.")
    return tag[1:]


def validate_tag(tag, package_version):
    if version(tag) != package_version:
        raise ValueError("Release tag must match the version in Cargo.toml.")
    return tag


def asset_name(tag, target):
    version(tag)
    if target not in TARGETS:
        raise ValueError("Unsupported release target.")
    return f"sculpt-{tag}-{target}.tar.gz"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def archive(tag, target, binary, output):
    name = asset_name(tag, target)
    binary, output = Path(binary), Path(output)
    if not binary.is_file() or binary.is_symlink():
        raise ValueError("Release binary must be a regular file.")
    output.mkdir(parents=True, exist_ok=True)
    destination = output / name
    if destination.is_symlink():
        raise ValueError("Archive destination must not be a symbolic link.")
    with tempfile.NamedTemporaryFile(dir=output, delete=False) as temporary:
        temporary_path = Path(temporary.name)
        try:
            with gzip.GzipFile(fileobj=temporary, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w|") as packed:
                    for entry_name, source in [(TARGETS[target], binary), *[(licence, ROOT / licence) for licence in sorted(LICENCES)]]:
                        info = tarfile.TarInfo(entry_name)
                        info.size = source.stat().st_size
                        info.mode = 0o755 if entry_name == TARGETS[target] else 0o644
                        with source.open("rb") as content:
                            packed.addfile(info, content)
            temporary.flush()
        except BaseException:
            temporary.close()
            temporary_path.unlink(missing_ok=True)
            raise
    temporary_path.replace(destination)
    return destination


def check_archive(path, binary):
    if not path.is_file() or path.is_symlink():
        raise ValueError("Every target requires a regular release archive.")
    try:
        with tarfile.open(path, "r:gz") as packed:
            names = set()
            for info in packed:
                if not info.isfile() or info.name in names or info.name not in {binary, *LICENCES} or info.size == 0:
                    raise ValueError("Archive must contain only its binary and both licence texts as regular files.")
                names.add(info.name)
                # Read each member fully so truncated payloads cannot produce installer metadata.
                with packed.extractfile(info) as content:
                    while content.read(1024 * 1024):
                        pass
            if names != {binary, *LICENCES}:
                raise ValueError("Archive is missing its binary or licence texts.")
    except (tarfile.TarError, EOFError, OSError) as error:
        raise ValueError("Invalid release archive.") from error


def render(name, values):
    text = (ROOT / "packaging" / f"{name}.in").read_text(encoding="utf-8")
    for key, value in values.items():
        text = text.replace(f"@{key}@", value)
    if re.search(r"@[A-Z_]+@", text):
        raise ValueError("Unresolved installer template value.")
    return text.encode()


def generate(tag, artifacts, output):
    number = version(tag)
    artifacts, output = Path(artifacts), Path(output)
    names = {asset_name(tag, target) for target in TARGETS}
    if not artifacts.is_dir() or {p.name for p in artifacts.iterdir()} - names - METADATA:
        raise ValueError("Artifact directory contains an unexpected release or file.")
    values = {"VERSION": number, "TAG": tag, "BASE_URL": f"{DOWNLOADS}/{tag}"}
    hashes = {}
    for target, binary in TARGETS.items():
        name = asset_name(tag, target)
        check_archive(artifacts / name, binary)
        hashes[name] = digest(artifacts / name)
        values[target.upper().replace("-", "_")] = hashes[name]
    files = {name: render(name, values) for name in ["sculpt.rb", "install.sh", "install.ps1"]}
    windows = "x86_64-pc-windows-msvc"
    files["sculpt.json"] = (json.dumps({
        "version": number,
        "description": "Version control and collaboration for 3D, on top of Git",
        "homepage": "https://github.com/Sculptary/cli",
        "license": "MIT|Apache-2.0",
        "architecture": {"64bit": {"url": f"{DOWNLOADS}/{tag}/{asset_name(tag, windows)}", "hash": hashes[asset_name(tag, windows)]}},
        "bin": "sculpt.exe",
    }, indent=2) + "\n").encode()
    hashes.update({name: hashlib.sha256(content).hexdigest() for name, content in files.items()})
    files["SHA256SUMS"] = "".join(f"{value}  {name}\n" for name, value in sorted(hashes.items())).encode()
    if output.exists() and {p.name for p in output.iterdir()} - names - METADATA:
        raise ValueError("Output directory contains unrelated files.")
    output.mkdir(parents=True, exist_ok=True)
    if any((output / name).is_symlink() for name in names | METADATA):
        raise ValueError("Release output files must not be symbolic links.")
    for name in sorted(names):
        if (artifacts / name).resolve() != (output / name).resolve():
            shutil.copyfile(artifacts / name, output / name)
    for name, content in files.items():
        (output / name).write_bytes(content)
    (output / "install.sh").chmod(0o755)
    verify(output)


def verify(directory):
    directory = Path(directory)
    seen = set()
    for line in (directory / "SHA256SUMS").read_text(encoding="ascii").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.+-]+)", line)
        if not match or match[2] in seen or match[2] == "SHA256SUMS":
            raise ValueError("Invalid release checksum manifest.")
        path = directory / match[2]
        if path.is_symlink() or not path.is_file() or digest(path) != match[1]:
            raise ValueError("Release file does not match its checksum.")
        seen.add(match[2])
    if seen != {p.name for p in directory.iterdir()} - {"SHA256SUMS"} or len(seen) != len(TARGETS) + len(METADATA) - 1:
        raise ValueError("Release checksum manifest is incomplete.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("tag")
    pack = commands.add_parser("archive")
    pack.add_argument("--tag", required=True)
    pack.add_argument("--target", choices=TARGETS, required=True)
    pack.add_argument("--binary", type=Path, required=True)
    pack.add_argument("--output", type=Path, required=True)
    generate_parser = commands.add_parser("generate")
    generate_parser.add_argument("--tag", required=True)
    generate_parser.add_argument("--artifacts", type=Path, required=True)
    generate_parser.add_argument("--output", type=Path, required=True)
    verification = commands.add_parser("verify")
    verification.add_argument("directory", type=Path)
    args = parser.parse_args()
    package_version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    try:
        if args.command == "tag":
            ref = os.environ.get("GITHUB_REF", "")
            tag = ref.removeprefix("refs/tags/") if ref.startswith("refs/tags/") else f"v{package_version}"
            print(validate_tag(tag, package_version))
        elif args.command == "verify":
            verify(args.directory)
        else:
            validate_tag(args.tag, package_version)
            if args.command == "archive":
                print(archive(args.tag, args.target, args.binary, args.output))
            else:
                generate(args.tag, args.artifacts, args.output)
    except (ValueError, OSError) as error:
        parser.exit(1, f"Release packaging failed: {error}\n")


if __name__ == "__main__":
    main()
