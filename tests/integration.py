"""Runs only --sandbox instances against synthetic history and file-backed OS adapters."""

import json
import pathlib
import subprocess
import sys
import tempfile
import time

exe = pathlib.Path(sys.argv[1]).resolve()
root = pathlib.Path(tempfile.mkdtemp(prefix="cvc-integration-"))
processes = []
passed = 0


def wait_for(predicate, description, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            if predicate():
                return
        except (FileNotFoundError, PermissionError, json.JSONDecodeError):
            pass
        time.sleep(0.05)
    raise AssertionError(description)


def check(name, fn):
    global passed
    fn()
    passed += 1
    print("PASS", name, flush=True)


def state():
    return json.loads((root / "state.json").read_text(encoding="utf-8-sig"))


def append(record_id, text):
    with (root / "history.jsonl").open("a", encoding="utf-8") as f:
        f.write(json.dumps({"id": record_id, "text": text}, ensure_ascii=False) + "\n")


def command(value):
    (root / "command.txt").write_text(value, encoding="utf-8")
    wait_for(lambda: not (root / "command.txt").exists(), "command was not consumed")


def copied(text):
    return (root / "clipboard.txt").read_text(encoding="utf-8-sig") == text


def assert_true(condition):
    assert condition


try:
    install = root / "install"
    installed = install / "CodexVoiceClipboard.exe"

    def sandbox_install():
        result = subprocess.run(
            [str(exe), "--install", "--sandbox", str(root)],
            timeout=20,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        assert result.returncode == 0
        assert installed.exists()
        assert (root / "registration.json").exists()
        assert not (root / "startup.txt").exists()

    check(
        "sandbox installer copies single EXE and registers only fake application",
        sandbox_install,
    )
    append("old", "起動前の履歴")
    proc = subprocess.Popen(
        [str(installed), "--sandbox", str(root)],
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    processes.append(proc)
    wait_for(lambda: state()["recentCount"] == 1, "startup baseline")
    check(
        "STA tray message loop starts with synthetic history",
        lambda: assert_true(state()["sta"]),
    )
    check(
        "old history is not copied on startup",
        lambda: assert_true(not (root / "clipboard.txt").exists()),
    )

    def duplicate():
        other = subprocess.Popen(
            [str(installed), "--sandbox", str(root)],
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        processes.append(other)
        assert other.wait(timeout=10) == 2
        assert proc.poll() is None

    check("second process exits without replacing resident", duplicate)

    def watcher_copy():
        wait_for(lambda: state()["watcherHealthy"], "FileSystemWatcher not active")
        append("new", "日本語\n二行目🙂")
        time.sleep(0.4)
        assert not (root / "clipboard.txt").exists(), "copied before delay"
        wait_for(lambda: copied("日本語\n二行目🙂"), "watcher copy")

    check(
        "watcher plus two-second delay copies Japanese multiline Unicode", watcher_copy
    )

    def duplicate_record():
        append("new", "同じIDを再コピーしない")
        time.sleep(3)
        assert copied("日本語\n二行目🙂")

    check("duplicate record does not trigger copy", duplicate_record)

    def pause_resume():
        command("pause")
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
        wait_for(
            lambda: copied("短い停止から再開した後"), "new record after quick resume"
        )

    check(
        "quick pause and resume drains unpolled paused history without replay",
        quick_resume,
    )

    def recreate():
        (root / "history.jsonl").unlink()
        wait_for(lambda: state()["historyError"], "missing history error")
        append("recreated", "再生成")
        wait_for(lambda: copied("再生成"), "recreated history copy")
        wait_for(lambda: not state()["historyError"], "history error cleared")

    check("deleted history errors and recreated file recovers", recreate)

    def clipboard_retry():
        (root / "clipboard-failures.txt").write_text("3", encoding="utf-8")
        append("retry", "競合後に復旧")
        wait_for(lambda: copied("競合後に復旧"), "clipboard contention recovery")

    check("transient clipboard adapter failures recover", clipboard_retry)

    def startup():
        command("startup-on")
        wait_for(lambda: (root / "startup.txt").exists(), "fake startup on")
        command("startup-off")
        wait_for(lambda: not (root / "startup.txt").exists(), "fake startup off")

    check("startup switches use sandbox file store", startup)

    def private_log():
        log = (root / "app.log").read_text(encoding="utf-8-sig")
        assert "clipboard-copied" in log
        for text in [
            "起動前の履歴",
            "日本語",
            "一時停止中",
            "競合後に復旧",
            "短い停止",
        ]:
            assert text not in log

    check(
        "executed application logs contain no synthetic transcript bodies", private_log
    )

    def uninstall():
        command("startup-on")
        command("uninstall")
        assert proc.wait(timeout=30) == 0
        wait_for(
            lambda: not install.exists(), "sandbox install files remain", seconds=45
        )
        assert not (root / "startup.txt").exists()
        assert not (root / "registration.json").exists()
        wait_for(
            lambda: (root / "uninstall-result.json").exists(), "uninstall worker result"
        )
        result = json.loads(
            (root / "uninstall-result.json").read_text(encoding="utf-8-sig")
        )
        assert result["success"]
        wait_for(lambda: not pid_alive(result["workerPid"]), "uninstall worker remains")
        wait_for(
            lambda: not pathlib.Path(result["workerDirectory"]).exists(),
            "temporary worker cleanup remains",
            seconds=30,
        )

    def pid_alive(pid):
        result = subprocess.run(
            ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
            capture_output=True,
            text=True,
            errors="replace",
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        return str(pid) in result.stdout

    check(
        "sandbox uninstall stops resident, removes startup and files, exits worker",
        uninstall,
    )

    # Separate process with watcher disabled proves the periodic path on its own.
    def polling_only():
        poll_root = root / "poll-only"
        poll_root.mkdir()
        (poll_root / "history.jsonl").write_text("", encoding="utf-8")
        poll_proc = subprocess.Popen(
            [str(exe), "--sandbox", str(poll_root), "--no-watcher"],
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        processes.append(poll_proc)
        wait_for(lambda: (poll_root / "state.json").exists(), "poll-only ready")
        (poll_root / "history.jsonl").write_text(
            json.dumps({"id": "poll", "text": "定期確認"}, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        wait_for(
            lambda: (poll_root / "clipboard.txt").read_text(encoding="utf-8-sig")
            == "定期確認",
            "poll-only copy",
        )
        (poll_root / "command.txt").write_text("exit", encoding="utf-8")
        assert poll_proc.wait(timeout=10) == 0

    check(
        "periodic polling alone recovers missed watcher events and exit ends process",
        polling_only,
    )
    print(f"RESULT: {passed} integration tests passed, 0 failed", flush=True)
finally:
    for process in processes:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
    # Evidence is copied by the caller before cleanup when needed; tests contain fake text only.
    print("Sandbox evidence:", root, flush=True)
