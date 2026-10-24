"""Runs only --sandbox instances against synthetic history and file-backed OS adapters.

Usage: python3 tests/integration.py <app executable or .app bundle>
Works on Windows and macOS. Never touches the real clipboard, login items or history.
"""

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

target = pathlib.Path(sys.argv[1]).resolve()
if target.suffix == ".app":
    target = next((target / "Contents" / "MacOS").iterdir())
exe = target
root = pathlib.Path(tempfile.mkdtemp(prefix="cvc-integration-"))
processes = []
passed = 0
FLAGS = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


def launch(*args):
    proc = subprocess.Popen([str(exe), *args], **FLAGS)
    processes.append(proc)
    return proc


def wait_for(predicate, description, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            if predicate():
                return
        except (FileNotFoundError, PermissionError, json.JSONDecodeError, KeyError):
            pass
        time.sleep(0.05)
    raise AssertionError(description)


def check(name, fn):
    global passed
    fn()
    passed += 1
    print("PASS", name, flush=True)


def state(base=None):
    return json.loads(((base or root) / "state.json").read_text(encoding="utf-8"))


def append(record_id, text, base=None):
    with ((base or root) / "history.jsonl").open("a", encoding="utf-8") as f:
        f.write(json.dumps({"id": record_id, "text": text}, ensure_ascii=False) + "\n")


def command(value):
    (root / "command.txt").write_text(value, encoding="utf-8")
    wait_for(lambda: not (root / "command.txt").exists(), "command was not consumed")


def copied(text, base=None):
    return ((base or root) / "clipboard.txt").read_text(encoding="utf-8") == text


def tray_pids():
    path = root / "tray-icons.log"
    return path.read_text().split() if path.exists() else []


try:
    # Stands in for the installed .app that the menu's uninstall deletes.
    (root / "app" / "Contents").mkdir(parents=True)
    append("old", "起動前の履歴")
    proc = launch("--sandbox", str(root))
    wait_for(lambda: state()["recentCount"] == 1, "startup baseline")

    def no_startup_copy():
        time.sleep(2.5)
        assert not (root / "clipboard.txt").exists(), "copied pre-launch history"

    check("old history is not copied on startup", no_startup_copy)

    def one_icon():
        wait_for(lambda: state()["trayIcons"] == 1, "tray icon not created")
        assert tray_pids() == [str(proc.pid)], tray_pids()

    check("resident puts up exactly one tray icon", one_icon)

    def duplicate():
        other = launch("--sandbox", str(root))
        other.wait(timeout=10)
        assert proc.poll() is None, "resident was replaced"
        assert tray_pids() == [str(proc.pid)], "second instance created an icon"

    check("second process exits without an icon and keeps the resident", duplicate)

    def watcher_copy():
        wait_for(lambda: state()["watcherHealthy"], "file watcher not active")
        append("new", "日本語\n二行目🙂")
        time.sleep(0.4)
        assert not (root / "clipboard.txt").exists(), "copied before delay"
        wait_for(lambda: copied("日本語\n二行目🙂"), "watcher copy")

    check("watcher plus two-second delay copies Japanese multiline Unicode", watcher_copy)

    def duplicate_record():
        append("new", "同じIDを再コピーしない")
        time.sleep(3)
        assert copied("日本語\n二行目🙂")

    check("duplicate record does not trigger copy", duplicate_record)

    def pause_resume():
        command("pause")
        wait_for(lambda: state()["state"] == "paused", "paused state")
        append("paused", "一時停止中")
        wait_for(lambda: state()["recentCount"] == 3, "paused history available")
        time.sleep(2.5)
        assert copied("日本語\n二行目🙂")
        command("manual")
        wait_for(lambda: copied("一時停止中"), "manual during pause")
        command("resume")
        append("resumed", "再開後")
        wait_for(lambda: copied("再開後"), "resume copy")

    check("pause keeps manual recovery and resume copies future records", pause_resume)

    def quick_resume():
        command("pause")
        append("quick-paused", "短い停止中の履歴")
        command("resume")
        time.sleep(3)
        assert copied("再開後"), "resume replayed a record from during the pause"
        append("quick-future", "短い停止から再開した後")
        wait_for(lambda: copied("短い停止から再開した後"), "new record after quick resume")

    check("quick pause and resume drains unpolled paused history without replay", quick_resume)

    def recreate():
        (root / "history.jsonl").unlink()
        wait_for(lambda: state()["historyError"], "missing history error")
        wait_for(lambda: state()["state"] == "error", "error state")
        append("recreated", "再生成")
        wait_for(lambda: copied("再生成"), "recreated history copy")
        wait_for(lambda: not state()["historyError"], "history error cleared")

    check("deleted history errors and recreated file recovers", recreate)

    def clipboard_retry():
        (root / "clipboard-failures.txt").write_text("3", encoding="utf-8")
        append("retry", "競合後に復旧")
        wait_for(lambda: copied("競合後に復旧"), "clipboard contention recovery")
        assert not state()["clipboardError"]

    check("transient clipboard adapter failures recover", clipboard_retry)

    def startup():
        command("startup-on")
        wait_for(lambda: (root / "startup.txt").exists(), "fake startup on")
        wait_for(lambda: state()["startup"] == "On", "startup shown on")
        command("startup-off")
        wait_for(lambda: not (root / "startup.txt").exists(), "fake startup off")

    check("startup switches use sandbox file store", startup)

    def startup_disabled_by_os():
        command("startup-on")
        (root / "startup-disabled-by-os.txt").write_text("", encoding="utf-8")
        wait_for(lambda: state()["startup"] == "DisabledByOs", "OS-disabled startup not detected")
        assert (root / "startup.txt").exists(), "registration was changed"
        (root / "startup-disabled-by-os.txt").unlink()
        wait_for(lambda: state()["startup"] == "On", "OS re-enable not detected")

    check("startup disabled in OS settings is shown and left alone", startup_disabled_by_os)

    def private_log():
        log = (root / "app.log").read_text(encoding="utf-8")
        assert "clipboard-copied" in log
        for line in log.splitlines():
            code = line.split(" ", 1)[1]
            assert code.replace("-", "").isalpha() and code.islower(), line
        for text in ["起動前の履歴", "日本語", "一時停止中", "競合後に復旧", "短い停止", "再生成"]:
            assert text not in log
        settings = (root / "settings.json").read_text(encoding="utf-8")
        assert "再開後" not in settings and "text" not in settings

    check("executed application logs and settings contain no transcript bodies", private_log)

    def uninstall_running():
        assert (root / "startup.txt").exists()
        result = subprocess.run([str(exe), "--uninstall", "--sandbox", str(root)], timeout=45, **FLAGS)
        assert result.returncode == 0
        assert proc.wait(timeout=10) == 0
        for gone in ["startup.txt", "settings.json", "app.log"]:
            assert not (root / gone).exists(), f"{gone} remains"
        assert (root / "history.jsonl").exists(), "history was removed"
        assert (root / "app").exists(), "--uninstall deleted the application itself"

    check("--uninstall stops the sandbox resident and removes only what it created", uninstall_running)

    def menu_uninstall():
        menu_root = root / "menu-uninstall"
        (menu_root / "app" / "Contents").mkdir(parents=True)
        (menu_root / "history.jsonl").write_text("", encoding="utf-8")
        (menu_root / "startup.txt").write_text("enabled", encoding="utf-8")
        menu_proc = launch("--sandbox", str(menu_root))
        wait_for(lambda: state(menu_root)["state"] == "active", "menu-uninstall resident ready")
        (menu_root / "command.txt").write_text("uninstall", encoding="utf-8")
        assert menu_proc.wait(timeout=20) == 0
        for gone in ["startup.txt", "settings.json", "app.log", "app"]:
            assert not (menu_root / gone).exists(), f"{gone} remains"
        assert (menu_root / "history.jsonl").exists(), "history was removed"

    check("menu uninstall also deletes the application", menu_uninstall)

    def uninstall_alone():
        alone = root / "uninstall-alone"
        alone.mkdir()
        for name in ["settings.json", "app.log", "startup.txt", "history.jsonl"]:
            (alone / name).write_text("{}", encoding="utf-8")
        result = subprocess.run([str(exe), "--uninstall", "--sandbox", str(alone)], timeout=20, **FLAGS)
        assert result.returncode == 0
        assert sorted(p.name for p in alone.iterdir() if p.name != "instance.lock") == ["history.jsonl"]
        assert not (alone / "tray-icons.log").exists(), "uninstall created a tray icon"

    check("--uninstall without a resident cleans up and creates no icon", uninstall_alone)

    def polling_only():
        poll_root = root / "poll-only"
        poll_root.mkdir()
        (poll_root / "history.jsonl").write_text("", encoding="utf-8")
        poll_proc = launch("--sandbox", str(poll_root), "--no-watcher")
        wait_for(lambda: state(poll_root)["state"] == "active", "poll-only ready")
        assert not state(poll_root)["watcherHealthy"]
        (poll_root / "history.jsonl").write_text(
            json.dumps({"id": "poll", "text": "定期確認"}, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        wait_for(lambda: copied("定期確認", poll_root), "poll-only copy")
        (poll_root / "command.txt").write_text("exit", encoding="utf-8")
        assert poll_proc.wait(timeout=10) == 0

    check("periodic polling alone picks up changes and exit ends the process", polling_only)
    print(f"RESULT: {passed} integration tests passed, 0 failed", flush=True)
finally:
    for process in processes:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
    # Evidence stays for the caller; it holds synthetic text only.
    print("Sandbox evidence:", root, flush=True)
