"""Static guard for public release: license, one version everywhere, a downloadable build per OS."""

import json
import pathlib
import re
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
RELEASE = ROOT / ".github" / "workflows" / "release.yml"
CI = ROOT / ".github" / "workflows"


def read(path):
    return (ROOT / path).read_text(encoding="utf-8")


def tauri_conf():
    return json.loads(read("src-tauri/tauri.conf.json"))


class LicenseTests(unittest.TestCase):
    def test_license_file_is_mit_and_matches_cargo(self):
        self.assertTrue(read("LICENSE").startswith("MIT License"))
        workspace = tomllib.loads(read("Cargo.toml"))["workspace"]["package"]
        self.assertEqual("MIT", workspace["license"])
        self.assertEqual("MIT", json.loads(read("package.json"))["license"])


class VersionTests(unittest.TestCase):
    def test_every_manifest_carries_the_same_version(self):
        versions = {
            "tauri.conf.json": tauri_conf()["version"],
            "package.json": json.loads(read("package.json"))["version"],
            "src-tauri/Cargo.toml": tomllib.loads(read("src-tauri/Cargo.toml"))["package"]["version"],
            "core/Cargo.toml": tomllib.loads(read("core/Cargo.toml"))["package"]["version"],
        }
        self.assertEqual(1, len(set(versions.values())), versions)


class MacBundleTests(unittest.TestCase):
    def test_app_is_ad_hoc_signed(self):
        # Without a signature, a downloaded arm64 .app is reported as "damaged" instead of
        # offering "Open Anyway" in Privacy & Security.
        self.assertEqual("-", tauri_conf()["bundle"]["macOS"].get("signingIdentity"))


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.text = RELEASE.read_text(encoding="utf-8")
        # The release reuses the per-OS CI workflows, so their steps are part of it.
        called = re.findall(r"uses:\s*\./\.github/workflows/(\S+)", self.text)
        self.assertEqual(["windows.yml", "macos.yml"], called)
        self.pipeline = self.text + "".join((CI / name).read_text(encoding="utf-8") for name in called)

    def test_runs_only_on_version_tags(self):
        on = re.search(r"^on:\n(.*?)^\S", self.text, re.S | re.M).group(1)
        self.assertIn("tags:", on)
        self.assertIn('"v*"', on)
        self.assertNotIn("branches:", on)

    def test_called_workflows_accept_workflow_call(self):
        for name in ("windows.yml", "macos.yml"):
            self.assertIn("workflow_call:", (CI / name).read_text(encoding="utf-8"))

    def test_builds_installer_and_dmg_on_their_own_os(self):
        self.assertIn("windows-latest", self.pipeline)
        self.assertRegex(self.pipeline, r"macos-\d+|macos-latest")
        self.assertIn("--bundles nsis", self.pipeline)
        self.assertIn("--bundles app,dmg", self.pipeline)

    def test_tests_gate_the_release(self):
        for step in ("cargo test --workspace", "tests/integration.py", "codesign --verify"):
            self.assertIn(step, self.pipeline)
        self.assertIn("needs: [windows, macos]", self.text)

    def test_tag_must_match_app_version(self):
        self.assertIn("GITHUB_REF_NAME", self.text)
        self.assertIn("tauri.conf.json", self.text)

    def test_release_stays_a_draft_for_the_owner_to_publish(self):
        self.assertIn("gh release create", self.text)
        self.assertIn("--draft", self.text)

    def test_only_the_publishing_job_can_write(self):
        self.assertRegex(self.text, re.compile(r"^permissions:\n  contents: read", re.M))
        self.assertEqual(1, self.text.count("contents: write"))

    def test_concurrency_groups_differ_per_os(self):
        # Called workflows see the caller's github.workflow; a shared group would cancel one OS.
        groups = [re.search(r"group:\s*(\S+)", (CI / n).read_text(encoding="utf-8")).group(1)
                  for n in ("windows.yml", "macos.yml")]
        self.assertEqual(2, len(set(groups)))
        for group in groups:
            self.assertNotIn("github.workflow", group)

    def test_third_party_actions_are_pinned_by_sha(self):
        for workflow in CI.glob("*.yml"):
            for ref in re.findall(r"uses:\s*([^\s#]+)", workflow.read_text(encoding="utf-8")):
                if ref.startswith("./"):
                    continue
                with self.subTest(workflow=workflow.name, ref=ref):
                    self.assertRegex(ref, r"@[0-9a-f]{40}$")


class MacCiTests(unittest.TestCase):
    def test_pull_requests_run_the_sandbox_tests_on_macos(self):
        text = (CI / "macos.yml").read_text(encoding="utf-8")
        self.assertIn("pull_request", text)
        self.assertIn("tests/integration.py", text)
        self.assertIn("codesign --verify", text)


if __name__ == "__main__":
    unittest.main()
