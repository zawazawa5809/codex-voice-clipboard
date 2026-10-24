"""Static guard for spec 2 "削除": the Windows uninstaller leaves nothing the app created."""

import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HOOKS = ROOT / "src-tauri" / "windows" / "hooks.nsh"


def macro_body(text, name):
    match = re.search(rf"!macro {name}\s*\n(.*?)!macroend", text, re.S)
    return match.group(1) if match else None


class InstallerHookTests(unittest.TestCase):
    def setUp(self):
        self.hooks = HOOKS.read_text(encoding="utf-8")
        conf = json.loads(
            (ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8")
        )
        self.hook_file = conf["bundle"]["windows"]["nsis"]["installerHooks"]

    def test_tauri_conf_points_at_this_hook_file(self):
        self.assertEqual(HOOKS, (ROOT / "src-tauri" / self.hook_file).resolve())

    def test_pre_uninstall_runs_the_app_cleanup_without_asking_again(self):
        body = macro_body(self.hooks, "NSIS_HOOK_PREUNINSTALL")
        self.assertIsNotNone(body)
        self.assertIn(
            "ExecWait '\"$INSTDIR\\codex-voice-clipboard.exe\" --uninstall --assume-yes'",
            body,
        )

    def test_post_uninstall_removes_the_webview_data_unless_updating(self):
        # WebView2 keeps its profile in %LOCALAPPDATA%\<identifier>. Tauri's uninstaller
        # removes it only when the user ticks "delete app data", which is off by default.
        body = macro_body(self.hooks, "NSIS_HOOK_POSTUNINSTALL")
        self.assertIsNotNone(body)
        self.assertRegex(body, r"\$\{If\} \$UpdateMode <> 1")
        self.assertIn('RMDir /r "$LOCALAPPDATA\\${BUNDLEID}"', body)

    def test_build_fails_rather_than_removing_all_of_localappdata(self):
        # An empty BUNDLEID would turn the removal into RMDir /r "$LOCALAPPDATA\".
        body = macro_body(self.hooks, "NSIS_HOOK_POSTUNINSTALL")
        guard = body.find('!if "${BUNDLEID}" == ""')
        self.assertNotEqual(-1, guard)
        self.assertLess(body.find("!ifndef BUNDLEID"), guard)
        self.assertNotEqual(-1, body.find("!ifndef BUNDLEID"))
        self.assertIn("!error", body[guard:])
        self.assertLess(guard, body.index("RMDir /r"))


if __name__ == "__main__":
    unittest.main()
