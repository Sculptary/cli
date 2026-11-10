"""Exercise Homebrew with a local fixture tap and the actual compiled CLI."""
import functools
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import threading
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import release


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def main():
    binary = Path(sys.argv[1]).resolve()
    tag = "v" + tomllib.loads((release.ROOT / "Cargo.toml").read_text())["package"]["version"]
    environment = {**os.environ, "HOMEBREW_NO_AUTO_UPDATE": "1", "HOMEBREW_NO_INSTALL_FROM_API": "1", "HOMEBREW_NO_ANALYTICS": "1", "HOMEBREW_NO_ENV_HINTS": "1"}
    tap_name = "sculptary/cli-packaging-qa"
    formula = f"{tap_name}/sculpt"
    if tap_name in subprocess.check_output(["brew", "tap"], text=True, env=environment).splitlines():
        raise RuntimeError("The temporary packaging tap already exists.")
    with tempfile.TemporaryDirectory(prefix="sculpt-homebrew-") as directory:
        directory = Path(directory)
        artifacts, output = directory / "artifacts", directory / "release"
        target = "aarch64-apple-darwin" if platform.machine() in {"arm64", "aarch64"} else "x86_64-apple-darwin"
        for architecture in release.TARGETS:
            # Only the current native target is installed. Other archives are local fixtures.
            source = binary if architecture == target else directory / "fixture"
            if source != binary: source.write_bytes(b"Other platform fixture")
            release.archive(tag, architecture, source, artifacts)
        release.generate(tag, artifacts, output)
        server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(output)))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        tap = directory / "tap"
        (tap / "Formula").mkdir(parents=True)
        text = (output / "sculpt.rb").read_text().replace(f"{release.DOWNLOADS}/{tag}", f"http://127.0.0.1:{server.server_port}")
        (tap / "Formula/sculpt.rb").write_text(text)
        tapped = False
        try:
            subprocess.run(["git", "init", "-q", str(tap)], check=True)
            subprocess.run(["git", "-C", str(tap), "add", "Formula/sculpt.rb"], check=True)
            subprocess.run(["git", "-C", str(tap), "-c", "user.name=Packaging fixture", "-c", "user.email=packaging@example.invalid", "commit", "-qm", "Add local fixture formula"], check=True)
            subprocess.run(["brew", "tap", tap_name, tap.as_uri()], env=environment, check=True)
            tapped = True
            subprocess.run(["brew", "install", "--formula", formula], env=environment, check=True)
            subprocess.run(["brew", "test", formula], env=environment, check=True)
            print("Generated Homebrew formula installed and tested the native CLI.")
        finally:
            if tapped:
                subprocess.run(["brew", "uninstall", "--force", formula], env=environment, check=False)
                subprocess.run(["brew", "untap", tap_name], env=environment, check=False)
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == "__main__":
    main()
