using System.Text;
using System.Text.Json;
using CodexVoiceClipboard.Core;

// Dependency-free executable tests. Every failure exits nonzero; no real clipboard,
// registry, startup registration, installation, or Codex history is used.
internal static class Program
{
    private static int passed;
    [STAThread]
    private static int Main()
    {
        Run("startup history is available manually but never auto-copied", () => WithHistory((path, session) =>
        {
            Write(path, "old", "古い履歴");
            Equal(0, session.Poll().NewRecords.Count);
            Equal("old", session.Recent[0].Id);
            Append(path, "new", "新しい文字起こし");
            Equal("new", session.Poll().NewRecords.Single().Id);
        }));
        Run("same id is processed only once, including after file replacement", () => WithHistory((path, session) =>
        {
            Write(path, "old", "old"); session.Poll();
            Append(path, "n", "日本語"); Equal(1, session.Poll().NewRecords.Count);
            Append(path, "n", "duplicate"); Equal(0, session.Poll().NewRecords.Count);
            File.Delete(path); Write(path, "n", "duplicate"); Equal(0, session.Poll().NewRecords.Count);
        }));
        Run("partial JSON and split UTF8 are retried", () => WithHistory((path, session) =>
        {
            File.WriteAllText(path, ""); session.Poll();
            var bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new { id = "split", text = "日本語🙂" }, new JsonSerializerOptions { Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping }));
            var cut = Array.IndexOf(bytes, (byte)0xe6) + 1;
            // Serialization escapes Unicode by default, so split arbitrarily if no raw UTF8 exists.
            if (cut < 2) cut = bytes.Length / 2;
            File.WriteAllBytes(path, bytes[..cut]); Equal(0, session.Poll().NewRecords.Count);
            using (var file = new FileStream(path, FileMode.Append)) file.Write(bytes[cut..]);
            Equal("日本語🙂", session.Poll().NewRecords.Single().Text);
            Equal(0, session.Poll().NewRecords.Count);
        }));
        Run("malformed complete lines do not block subsequent records", () => WithHistory((path, session) =>
        {
            File.WriteAllText(path, ""); session.Poll();
            File.AppendAllText(path, "invalid\n{\"id\":\"missing-text\"}\n"); Append(path, "ok", "recover");
            var batch = session.Poll(); Equal(1, batch.NewRecords.Count); True(batch.InvalidLines >= 2);
        }));
        Run("Japanese long multiline and Unicode remain exact", () => WithHistory((path, session) =>
        {
            File.WriteAllText(path, ""); session.Poll();
            var text = string.Concat(Enumerable.Repeat("日本語𠮷👩🏽‍💻\r\n二行目\n", 30000));
            Append(path, "long", text); Equal(text, session.Poll().NewRecords.Single().Text);
        }));
        Run("missing at startup then creation emits new records", () => WithHistory((path, session) =>
        {
            Throws<FileNotFoundException>(() => session.Poll());
            Write(path, "created", "created"); Equal(1, session.Poll().NewRecords.Count);
        }));
        Run("delete and recreate, including equal length, recovers", () => WithHistory((path, session) =>
        {
            Write(path, "one", "one"); session.Poll(); File.Delete(path);
            Throws<FileNotFoundException>(() => session.Poll());
            Write(path, "two", "two"); Equal("two", session.Poll().NewRecords.Single().Id);
        }));
        Run("truncation is recovered", () => WithHistory((path, session) =>
        {
            Write(path, "long-initial", new string('x', 100)); session.Poll();
            Write(path, "tiny", "x"); Equal("tiny", session.Poll().NewRecords.Single().Id);
        }));
        Run("same-length in-place rewrite is recovered", () => WithHistory((path, session) =>
        {
            Write(path, "one", "one"); session.Poll();
            Write(path, "two", "two"); Equal("two", session.Poll().NewRecords.Single().Id);
        }));
        Run("initial lock cannot replay old history after unlock", () => WithHistory((path, session) =>
        {
            Write(path, "old", "old");
            using (var file = new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
                Throws<IOException>(() => session.Poll());
            Equal(0, session.Poll().NewRecords.Count);
            Append(path, "fresh", "fresh"); Equal(1, session.Poll().NewRecords.Count);
        }));
        Run("recent history is capped to ten, newest first", () => WithHistory((path, session) =>
        {
            File.WriteAllText(path, ""); session.Poll();
            for (int i = 0; i < 15; i++) Append(path, i.ToString(), "text");
            Equal(15, session.Poll().NewRecords.Count); Equal(10, session.Recent.Count); Equal("14", session.Recent[0].Id);
        }));
        Run("reader never changes source bytes", () => WithHistory((path, session) =>
        {
            Write(path, "a", "source"); var before = File.ReadAllBytes(path); session.Poll(); session.Poll();
            True(before.SequenceEqual(File.ReadAllBytes(path)));
        }));
        Run("copy waits two seconds and retries contention", () =>
        {
            var clipboard = new FakeClipboard { Failures = 2 }; var engine = new CopyCoordinator(clipboard);
            var now = DateTimeOffset.UtcNow; engine.Observe([new("a", "text")], now);
            engine.Tick(now.AddMilliseconds(1999)); Equal(0, clipboard.Attempts);
            engine.Tick(now.AddSeconds(2)); Equal(1, clipboard.Attempts);
            engine.Tick(now.AddSeconds(3)); engine.Tick(now.AddSeconds(4));
            Equal("text", clipboard.Text); True(!engine.HasError);
        });
        Run("pause cancels pending copy; manual recovery still works; resume does not replay", () =>
        {
            var clipboard = new FakeClipboard(); var engine = new CopyCoordinator(clipboard); var now = DateTimeOffset.UtcNow;
            engine.Observe([new("a", "first")], now); engine.Enabled = false;
            engine.Tick(now.AddSeconds(10)); Equal(0, clipboard.Attempts);
            engine.CopyManually(new("a", "first"), now); engine.Tick(now); Equal("first", clipboard.Text);
            engine.Observe([new("b", "paused")], now); engine.Enabled = true;
            engine.Tick(now.AddSeconds(20)); Equal(1, clipboard.Attempts);
            engine.Observe([new("c", "after resume")], now); engine.Tick(now.AddSeconds(2)); Equal("after resume", clipboard.Text);
        });
        Run("newest pending record replaces older pending record", () =>
        {
            var clipboard = new FakeClipboard(); var engine = new CopyCoordinator(clipboard); var now = DateTimeOffset.UtcNow;
            engine.Observe([new("a", "a"), new("b", "b")], now); engine.Tick(now.AddSeconds(2)); Equal("b", clipboard.Text); Equal(1, clipboard.Attempts);
        });
        Run("delay is configurable", () =>
        {
            var clipboard = new FakeClipboard(); var engine = new CopyCoordinator(clipboard) { Delay = TimeSpan.FromSeconds(5) }; var now = DateTimeOffset.UtcNow;
            engine.Observe([new("a", "a")], now); engine.Tick(now.AddSeconds(2)); Equal(0, clipboard.Attempts);
            engine.Tick(now.AddSeconds(5)); Equal("a", clipboard.Text);
        });
        Run("clipboard retries are bounded and manual retry recovers", () =>
        {
            var clipboard = new FakeClipboard { Failures = 100 }; var engine = new CopyCoordinator(clipboard); var now = DateTimeOffset.UtcNow;
            engine.CopyManually(new("a", "a"), now);
            for (int i = 0; i < 20; i++) engine.Tick(now.AddSeconds(i));
            Equal(8, clipboard.Attempts); True(engine.HasError);
            clipboard.Failures = 0; engine.CopyManually(new("a", "a"), now.AddSeconds(30)); engine.Tick(now.AddSeconds(30));
            Equal("a", clipboard.Text); True(!engine.HasError);
        });
        Run("manual copy cancels delayed automatic overwrite", () =>
        {
            var clipboard = new FakeClipboard(); var engine = new CopyCoordinator(clipboard); var now = DateTimeOffset.UtcNow;
            engine.Observe([new("auto", "auto")], now); engine.CopyManually(new("manual", "manual"), now);
            engine.Tick(now); engine.Tick(now.AddSeconds(10)); Equal("manual", clipboard.Text); Equal(1, clipboard.Attempts);
        });
        Run("logs contain event codes only, never supplied transcript", () =>
        {
            var root = NewRoot(); try { var log = new SafeLog(Path.Combine(root, "app.log")); log.Write("clipboard-copied");
                var text = File.ReadAllText(log.Path); True(text.Contains("clipboard-copied")); True(!text.Contains("日本語"));
                Throws<ArgumentException>(() => log.Write("secret transcript 日本語")); }
            finally { Directory.Delete(root, true); }
        });
        Run("startup toggles use injected store only", () =>
        {
            var store = new FakeStartup(); var startup = new StartupController(store, "C:\\test app\\app.exe");
            startup.SetEnabled(true); True(startup.Enabled); Equal("\"C:\\test app\\app.exe\"", store.Command);
            startup.SetEnabled(false); True(!startup.Enabled); Equal(2, store.Writes);
        });
        Run("uninstall stops process before removing startup, registration and files", () =>
        {
            var ops = new FakeUninstall(); new UninstallCoordinator(ops).Execute();
            Equal("stop,disable,unregister,delete", string.Join(',', ops.Calls));
        });
        Run("uninstall refuses deletion when resident fails to exit", () =>
        {
            var ops = new FakeUninstall { StopSucceeds = false }; Throws<IOException>(() => new UninstallCoordinator(ops).Execute());
            Equal("stop", string.Join(',', ops.Calls));
        });
        Console.WriteLine($"RESULT: {passed} passed, 0 failed"); return 0;
    }
    private static void Run(string name, Action test) { try { test(); passed++; Console.WriteLine($"PASS {name}"); } catch (Exception ex) { Console.Error.WriteLine($"FAIL {name}: {ex}"); Environment.Exit(1); } }
    private static void Equal<T>(T expected, T actual) { if (!EqualityComparer<T>.Default.Equals(expected, actual)) throw new Exception($"Expected {expected}, got {actual}"); }
    private static void True(bool value) { if (!value) throw new Exception("Assertion failed"); }
    private static void Throws<T>(Action action) where T : Exception { try { action(); } catch (T) { return; } throw new Exception($"Expected {typeof(T).Name}"); }
    private static string NewRoot() { var path = Path.Combine(Path.GetTempPath(), "cvc-test-" + Guid.NewGuid()); Directory.CreateDirectory(path); return path; }
    private static void WithHistory(Action<string, HistorySession> test) { var root = NewRoot(); try { var path = Path.Combine(root, "history.jsonl"); using var session = new HistorySession(path); test(path, session); } finally { Directory.Delete(root, true); } }
    private static void Write(string path, string id, string text) => File.WriteAllText(path, JsonSerializer.Serialize(new { id, text }) + "\n", new UTF8Encoding(false));
    private static void Append(string path, string id, string text) => File.AppendAllText(path, JsonSerializer.Serialize(new { id, text }) + "\n", new UTF8Encoding(false));
    private sealed class FakeClipboard : IClipboard { public int Failures; public int Attempts; public string? Text; public void SetText(string text) { Attempts++; if (Failures-- > 0) throw new System.Runtime.InteropServices.ExternalException(); Text = text; } }
    private sealed class FakeStartup : IStartupStore { public string? Command { get; set; } public int Writes; public void Write(string? command) { Command = command; Writes++; } }
    private sealed class FakeUninstall : IUninstallOperations { public List<string> Calls = []; public bool StopSucceeds = true; public bool StopResident() { Calls.Add("stop"); return StopSucceeds; } public void DisableStartup() => Calls.Add("disable"); public void RemoveRegistration() => Calls.Add("unregister"); public void DeleteApplication() => Calls.Add("delete"); }
}
