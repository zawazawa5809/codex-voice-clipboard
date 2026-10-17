using System.Diagnostics;
using System.Text;
using CodexVoiceClipboard.Core;
using Microsoft.Win32;

namespace CodexVoiceClipboard;

internal static class Installation
{
    private const string UninstallKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexVoiceClipboard";

    public static int Install(AppPaths paths)
    {
        if (!IsSingleFile()) throw new InvalidOperationException("Publish as single file before installing.");
        if (paths.Sandbox is null && MessageBox.Show(
            $"次の場所にインストールし、「インストールされているアプリ」に登録します。\n\n{paths.InstallDirectory}\n\n自動起動は初期状態ではOFFです。インストールしますか？",
            "Codex Voice Clipboard のインストール", MessageBoxButtons.YesNo, MessageBoxIcon.Question) != DialogResult.Yes) return 0;
        using var stopped = AcquireStoppedResident(paths) ?? throw new IOException("Resident is still running.");
        CheckDirectory(paths.InstallDirectory);
        Directory.CreateDirectory(paths.InstallDirectory);
        string source = Environment.ProcessPath!;
        if (!string.Equals(source, paths.InstalledExe, StringComparison.OrdinalIgnoreCase))
        {
            string pending = paths.InstalledExe + ".new";
            File.Copy(source, pending, true);
            File.Move(pending, paths.InstalledExe, true);
        }
        if (paths.Sandbox is null)
        {
            using var key = Registry.CurrentUser.CreateSubKey(UninstallKey);
            key.SetValue("DisplayName", "Codex Voice Clipboard");
            key.SetValue("DisplayVersion", "1.0.0");
            key.SetValue("Publisher", "Codex Voice Clipboard contributors");
            key.SetValue("InstallLocation", paths.InstallDirectory);
            key.SetValue("DisplayIcon", paths.InstalledExe);
            key.SetValue("UninstallString", $"\"{paths.InstalledExe}\" --uninstall");
            key.SetValue("NoModify", 1, RegistryValueKind.DWord);
            key.SetValue("NoRepair", 1, RegistryValueKind.DWord);
            key.SetValue("EstimatedSize", (int)(new FileInfo(paths.InstalledExe).Length / 1024), RegistryValueKind.DWord);
            MessageBox.Show($"インストールしました。\n\n起動するには次のEXEを開いてください。\n{paths.InstalledExe}\n\n自動起動はトレイメニューから設定できます。", "Codex Voice Clipboard", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }
        else paths.WriteJson("registration.json", new { name = "Codex Voice Clipboard" });
        return 0;
    }

    public static void StartUninstall(AppPaths paths)
    {
        if (!IsSingleFile()) throw new InvalidOperationException("Uninstall from the published application.");
        var temporary = Path.Combine(Path.GetTempPath(), "CodexVoiceClipboard-remove-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(temporary);
        var workerExe = Path.Combine(temporary, "CodexVoiceClipboard.exe");
        try
        {
            File.Copy(Environment.ProcessPath!, workerExe);
            var start = new ProcessStartInfo(workerExe) { UseShellExecute = false, CreateNoWindow = true };
            start.ArgumentList.Add("--remove");
            if (paths.Sandbox is not null) { start.ArgumentList.Add("--sandbox"); start.ArgumentList.Add(paths.Sandbox); }
            using var worker = Process.Start(start) ?? throw new IOException("Cannot start removal worker.");
        }
        catch { File.Delete(workerExe); Directory.Delete(temporary); throw; }
    }

    public static int Remove(AppPaths paths)
    {
        try
        {
            // The confirmation lives in the worker, so --remove can never bypass it in production.
            if (paths.Sandbox is null && MessageBox.Show(
                "Codex Voice Clipboardをアンインストールしますか？\n\n常駐を終了し、自動起動設定・アプリ登録・アプリファイル・このアプリの設定とログを削除します。\nCodexの履歴と現在のクリップボードは変更しません。",
                "アンインストールの確認", MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2) != DialogResult.Yes) return 0;
            using var operations = new RemovalOperations(paths);
            new UninstallCoordinator(operations).Execute();
            if (paths.Sandbox is not null) paths.WriteJson("uninstall-result.json", new { success = true, workerPid = Environment.ProcessId, workerDirectory = Path.GetDirectoryName(Environment.ProcessPath!) });
            return 0;
        }
        finally { ScheduleWorkerCleanup(); }
    }

    private sealed class RemovalOperations(AppPaths paths) : IUninstallOperations, IDisposable
    {
        private StoppedResident? stopped;
        public bool StopResident() { stopped = AcquireStoppedResident(paths); return stopped is not null; }
        public void Dispose() => stopped?.Dispose();
        public void DisableStartup()
        {
            if (paths.Sandbox is null)
            {
                var store = new RegistryStartupStore(); store.Write(null); store.RemoveApproval();
            }
            else new SandboxStartupStore(paths).Write(null);
        }
        public void RemoveRegistration()
        {
            if (paths.Sandbox is null) Registry.CurrentUser.DeleteSubKeyTree(UninstallKey, throwOnMissingSubKey: false);
            else File.Delete(Path.Combine(paths.StateDirectory, "registration.json"));
        }
        public void DeleteApplication()
        {
            CheckDirectory(paths.InstallDirectory);
            // Windows may briefly retain the just-stopped resident's image handle.
            for (int attempt = 0; ; attempt++)
            {
                try { if (Directory.Exists(paths.InstallDirectory)) Directory.Delete(paths.InstallDirectory, recursive: true); break; }
                catch (Exception ex) when ((ex is IOException or UnauthorizedAccessException) && attempt < 39) { Thread.Sleep(250); }
            }
            if (paths.Sandbox is null && Directory.Exists(paths.StateDirectory))
            {
                CheckDirectory(paths.StateDirectory);
                Directory.Delete(paths.StateDirectory, recursive: true);
            }
        }
    }

    private static StoppedResident? AcquireStoppedResident(AppPaths paths)
    {
        if (EventWaitHandle.TryOpenExisting(paths.StopName, out var signal))
        { using (signal) signal.Set(); }
        var mutex = new Mutex(false, paths.MutexName);
        bool acquired;
        try { acquired = mutex.WaitOne(TimeSpan.FromSeconds(15)); } catch (AbandonedMutexException) { acquired = true; }
        if (acquired) return new StoppedResident(mutex);
        mutex.Dispose(); return null;
    }
    private sealed class StoppedResident(Mutex mutex) : IDisposable
    {
        public void Dispose() { mutex.ReleaseMutex(); mutex.Dispose(); }
    }

    private static void CheckDirectory(string path)
    {
        // Do not follow a directory junction into an unrelated location during removal.
        for (var current = new DirectoryInfo(Path.GetFullPath(path)); current is not null; current = current.Parent)
            if (current.Exists && current.Attributes.HasFlag(FileAttributes.ReparsePoint)) throw new IOException("Reparse point not permitted.");
    }

    private static void ScheduleWorkerCleanup()
    {
        var executable = Environment.ProcessPath!;
        var directory = Path.GetDirectoryName(executable)!;
        var parent = Path.GetDirectoryName(directory);
        // Internal --remove invoked on the installed EXE must never delete its own parent this way.
        if (!string.Equals(parent?.TrimEnd(Path.DirectorySeparatorChar), Path.GetTempPath().TrimEnd(Path.DirectorySeparatorChar), StringComparison.OrdinalIgnoreCase)
            || !Path.GetFileName(directory).StartsWith("CodexVoiceClipboard-remove-", StringComparison.Ordinal)
            || !Guid.TryParseExact(Path.GetFileName(directory)["CodexVoiceClipboard-remove-".Length..], "N", out _)) return;
        var quotedDirectory = "'" + directory.Replace("'", "''", StringComparison.Ordinal) + "'";
        var script = $"$ErrorActionPreference='Stop'; Wait-Process -Id {Environment.ProcessId} -Timeout 60 -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 300; Remove-Item -LiteralPath {quotedDirectory} -Recurse -Force";
        var start = new ProcessStartInfo(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), @"WindowsPowerShell\v1.0\powershell.exe"))
        { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
        start.ArgumentList.Add("-NoProfile"); start.ArgumentList.Add("-NonInteractive"); start.ArgumentList.Add("-WindowStyle"); start.ArgumentList.Add("Hidden");
        start.ArgumentList.Add("-EncodedCommand"); start.ArgumentList.Add(Convert.ToBase64String(Encoding.Unicode.GetBytes(script)));
        try { using var cleanup = Process.Start(start); }
        catch (System.ComponentModel.Win32Exception) { /* The temp worker is harmless; app and startup are already removed. */ }
    }

    private static bool IsSingleFile()
    {
#pragma warning disable IL3000 // Empty Location is precisely the bundle check.
        return string.IsNullOrEmpty(typeof(Installation).Assembly.Location);
#pragma warning restore IL3000
    }
}
