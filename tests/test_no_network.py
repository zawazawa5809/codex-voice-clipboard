"""Static guard for spec 1.7 / 2: no network crates, no updater, CSP blocks external hosts."""

import json
import pathlib
import re
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
FORBIDDEN = ("reqwest", "hyper", "ureq", "isahc", "curl", "tauri-plugin-updater", "tauri-plugin-http")


def built_crates(target):
    out = subprocess.run(
        ["cargo", "tree", "-p", "codex-voice-clipboard", "-e", "normal", "--prefix", "none", "--target", target],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout
    return {line.split()[0] for line in out.splitlines() if line.strip()}


class NoNetworkTests(unittest.TestCase):
    def test_no_http_client_or_updater_is_built_for_either_os(self):
        for target in ("aarch64-apple-darwin", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target):
                self.assertEqual(set(), built_crates(target) & set(FORBIDDEN))

    def test_csp_allows_only_self_and_ipc(self):
        conf = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        csp = conf["app"]["security"]["csp"]
        directives = dict(
            (part.split()[0], part.split()[1:]) for part in csp.split(";") if part.strip()
        )
        self.assertEqual(["'self'"], directives["default-src"])
        self.assertEqual(["ipc:", "http://ipc.localhost"], directives["connect-src"])
        sources = [s for values in directives.values() for s in values]
        external = [s for s in sources if re.match(r"https?://", s) and s != "http://ipc.localhost"]
        self.assertEqual([], external)
        self.assertNotIn("*", sources)

    def test_frontend_loads_no_external_url(self):
        files = [ROOT / "index.html", *(ROOT / "src").glob("*.ts"), *(ROOT / "src").glob("*.css")]
        for file in files:
            with self.subTest(file=file.name):
                self.assertIsNone(re.search(r"https?://", file.read_text(encoding="utf-8")))


if __name__ == "__main__":
    unittest.main()
