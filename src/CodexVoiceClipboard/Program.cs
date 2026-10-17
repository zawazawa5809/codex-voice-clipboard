namespace CodexVoiceClipboard;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        ApplicationConfiguration.Initialize();
        try
        {
            string? sandbox = null;
            bool install = false, uninstall = false, remove = false, noWatcher = false;
            for (int i = 0; i < args.Length; i++)
            {
                switch (args[i])
                {
                    case "--sandbox" when i + 1 < args.Length: sandbox = args[++i]; break;
                    case "--install": install = true; break;
                    case "--uninstall": uninstall = true; break;
                    case "--remove": remove = true; break;
                    case "--no-watcher": noWatcher = true; break;
                    default: throw new ArgumentException("引数が正しくありません。");
                }
            }
            if ((install ? 1 : 0) + (uninstall ? 1 : 0) + (remove ? 1 : 0) > 1 || (noWatcher && sandbox is null))
                throw new ArgumentException("引数の組み合わせが正しくありません。");
            var paths = new AppPaths(sandbox);
            if (install) return Installation.Install(paths);
            if (uninstall) { Installation.StartUninstall(paths); return 0; }
            if (remove) return Installation.Remove(paths);
            using var instance = new SingleInstance(paths.MutexName);
            if (!instance.Acquired) return 2;
            Directory.CreateDirectory(paths.StateDirectory);
            using var context = new TrayContext(paths, noWatcher);
            Application.Run(context);
            return 0;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or InvalidOperationException or System.Security.SecurityException or System.ComponentModel.Win32Exception)
        {
            // Never show arbitrary exception messages that might contain a history fragment.
            MessageBox.Show("操作を完了できませんでした。ファイルの権限、アプリの実行状態、配置先を確認してください。", "Codex Voice Clipboard", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 1;
        }
    }
}
