using System.Diagnostics;
using CodexVoiceClipboard.Core;

namespace CodexVoiceClipboard;

internal sealed class TrayContext : ApplicationContext
{
    private readonly AppPaths paths;
    private readonly bool noWatcher;
    private readonly SafeLog log;
    private readonly HistorySession history;
    private readonly CopyCoordinator copies;
    private readonly IStartupStore startupStore;
    private readonly StartupController startup;
    private readonly NotifyIcon tray;
    private readonly ContextMenuStrip menu = new();
    private readonly ToolStripMenuItem statusItem = new() { Enabled = false };
    private readonly ToolStripMenuItem autoItem = new("自動コピー ON/OFF");
    private readonly ToolStripMenuItem latestItem = new("最新の文字起こしをコピー");
    private readonly ToolStripMenuItem recentItem = new("最近10件からコピー");
    private readonly ToolStripMenuItem startupItem = new("Windowsログイン時に自動起動");
    private readonly ToolStripMenuItem delayItem = new("コピーまでの待機時間…");
    private readonly System.Windows.Forms.Timer timer = new() { Interval = 100 };
    // A handle establishes the WinForms synchronization context for async polling.
    private readonly Control dispatcher = new();
    private readonly EventWaitHandle stopEvent;
    private readonly TrayIcons icons;
    private FileSystemWatcher? watcher;
    private IReadOnlyList<Transcript> recent = [];
    private AppSettings settings;
    private bool scanning, closing, historyError = true, resuming, commandBusy;
    private volatile bool watcherError;
    private readonly SemaphoreSlim historyGate = new(1, 1);
    private int dirty = 1;
    private int copyGeneration;
    private DateTimeOffset nextPoll = DateTimeOffset.MinValue;
    private string iconState = "";

    public TrayContext(AppPaths paths, bool noWatcher)
    {
        this.paths = paths; this.noWatcher = noWatcher;
        icons = new(paths.Sandbox is null && TrayIcons.DetectLightTaskbar());
        _ = dispatcher.Handle;
        log = new(paths.Log);
        settings = AppSettings.Load(paths);
        history = new(paths.History);
        copies = new(paths.Sandbox is null ? new WindowsClipboard() : new SandboxClipboard(paths))
        { Delay = TimeSpan.FromSeconds(settings.DelaySeconds), Enabled = settings.AutoCopy };
        copies.Event += code =>
        {
            log.Write(code);
            if (code == "clipboard-failed") ShowBalloon("コピーできませんでした。メニューから手動コピーを再試行できます。", ToolTipIcon.Error);
        };
        startupStore = paths.Sandbox is null ? new RegistryStartupStore() : new SandboxStartupStore(paths);
        startup = new(startupStore, paths.InstalledExe);
        stopEvent = new(false, EventResetMode.ManualReset, paths.StopName);
        stopEvent.Reset();
        tray = new() { Visible = true, Icon = icons.Error, Text = "Codex Voice Clipboard: 確認中", ContextMenuStrip = menu };
        menu.Items.AddRange([statusItem, new ToolStripSeparator(), autoItem, latestItem, recentItem, delayItem,
            new ToolStripSeparator(), startupItem, new ToolStripMenuItem("ログを表示", null, (_, _) => ShowLog()),
            new ToolStripMenuItem("クリップボード上書きについて", null, (_, _) => ShowRisk()),
            new ToolStripSeparator(), new ToolStripMenuItem("アンインストール…", null, (_, _) => StartUninstall()),
            new ToolStripMenuItem("終了", null, (_, _) => ExitThread())]);
        autoItem.Click += async (_, _) => await SetAuto(!copies.Enabled);
        latestItem.Click += (_, _) => { if (recent.Count > 0) ManualCopy(recent[0]); };
        startupItem.Click += (_, _) => ToggleStartup();
        delayItem.Click += (_, _) => EditDelay();
        menu.Opening += (_, _) => RefreshMenu();
        tray.DoubleClick += (_, _) => { if (recent.Count > 0) ManualCopy(recent[0]); };
        timer.Tick += OnTick;
        EnsureWatcher();
        timer.Start();
        log.Write("application-started");
        if (paths.Sandbox is null && !File.Exists(Path.Combine(paths.StateDirectory, "risk-notice-seen")))
        {
            ShowRisk();
            try { File.WriteAllText(Path.Combine(paths.StateDirectory, "risk-notice-seen"), "1"); }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { log.Write("settings-save-failed"); }
        }
    }

    private async void OnTick(object? sender, EventArgs e)
    {
        if (closing) return;
        if (stopEvent.WaitOne(0)) { ExitThread(); return; }
        if (commandBusy) return;
        if (paths.Sandbox is not null)
        {
            commandBusy = true;
            try { await ConsumeSandboxCommand(); } finally { commandBusy = false; }
        }
        if (closing) return;
        copies.Tick(DateTimeOffset.UtcNow);
        UpdateStatus();
        if (scanning || resuming) return;
        var now = DateTimeOffset.UtcNow;
        if (now < nextPoll && Interlocked.Exchange(ref dirty, 0) == 0) return;
        nextPoll = now.AddSeconds(2);
        Interlocked.Exchange(ref dirty, 0);
        EnsureWatcher();
        scanning = true;
        int generation = copyGeneration;
        try
        {
            var result = await ReadHistory();
            if (closing) return;
            recent = result.recent;
            historyError = false;
            if (generation == copyGeneration) copies.Observe(result.batch.NewRecords, DateTimeOffset.UtcNow);
            if (result.batch.InvalidLines > 0) log.Write("history-invalid-record");
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            if (!historyError) log.Write("history-unavailable");
            historyError = true;
        }
        finally
        {
            scanning = false;
            if (!closing) UpdateStatus();
        }
    }

    private void EnsureWatcher()
    {
        if (noWatcher) return;
        string directory = Path.GetDirectoryName(paths.History)!;
        if (!Directory.Exists(directory)) { watcher?.Dispose(); watcher = null; watcherError = true; return; }
        if (watcher is not null && !watcherError) return;
        watcher?.Dispose(); watcher = null;
        try
        {
            watcher = new FileSystemWatcher(directory, Path.GetFileName(paths.History))
            { NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite | NotifyFilters.Size | NotifyFilters.CreationTime };
            watcher.Changed += (_, _) => Interlocked.Exchange(ref dirty, 1);
            watcher.Created += (_, _) => Interlocked.Exchange(ref dirty, 1);
            watcher.Deleted += (_, _) => Interlocked.Exchange(ref dirty, 1);
            watcher.Renamed += (_, _) => Interlocked.Exchange(ref dirty, 1);
            watcher.Error += (_, _) => { watcherError = true; Interlocked.Exchange(ref dirty, 1); };
            watcher.EnableRaisingEvents = true;
            watcherError = false;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        { watcherError = true; log.Write("watcher-unavailable"); }
    }

    private void RefreshMenu()
    {
        autoItem.Checked = copies.Enabled;
        latestItem.Enabled = recent.Count > 0;
        recentItem.DropDownItems.Clear();
        recentItem.Enabled = recent.Count > 0;
        // Text appears only in this user-requested menu, never in disk logs or settings.
        for (int i = 0; i < recent.Count; i++)
        {
            var record = recent[i];
            var preview = record.Text.Replace('\r', ' ').Replace('\n', ' ').Replace("&", "&&", StringComparison.Ordinal);
            if (preview.Length > 64) preview = preview[..64] + "…";
            recentItem.DropDownItems.Add($"{i + 1}. {preview}", null, (_, _) => ManualCopy(record));
        }
        try
        {
            bool registered = startup.Enabled;
            bool disabled = startupStore is RegistryStartupStore registry && registry.DisabledByWindows;
            startupItem.Checked = registered && !disabled;
            startupItem.Text = disabled ? "自動起動: Windows設定で無効（設定を開く）" : "Windowsログイン時に自動起動";
            startupItem.Enabled = File.Exists(paths.InstalledExe);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.Security.SecurityException)
        { startupItem.Enabled = false; log.Write("startup-read-failed"); }
        delayItem.Text = $"コピーまでの待機時間…（{copies.Delay.TotalSeconds:0.0} 秒）";
        UpdateStatus();
    }
    private async Task<(HistoryBatch batch, Transcript[] recent)> ReadHistory()
    {
        await historyGate.WaitAsync();
        try { return await Task.Run(() => { var batch = history.Poll(); return (batch, history.Recent.ToArray()); }); }
        finally { historyGate.Release(); }
    }
    private async Task SetAuto(bool enabled)
    {
        int generation = ++copyGeneration;
        // Drain history while disabled before completing a resume. Even a quick pause/
        // append/resume cannot accidentally auto-copy records written during the pause.
        copies.Enabled = false;
        if (enabled)
        {
            resuming = true;
            try
            {
                var result = await ReadHistory();
                if (closing || generation != copyGeneration) return;
                recent = result.recent; historyError = false;
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { historyError = true; }
            finally { resuming = false; }
            if (closing || generation != copyGeneration) return;
            copies.Enabled = true;
        }
        settings = settings with { AutoCopy = enabled };
        SaveSettings();
        log.Write(enabled ? "auto-copy-enabled" : "auto-copy-paused");
        UpdateStatus();
    }
    private void ManualCopy(Transcript record)
    {
        copyGeneration++; // Prevent an in-flight automatic scan from overriding this selection.
        copies.CopyManually(record, DateTimeOffset.UtcNow);
        copies.Tick(DateTimeOffset.UtcNow);
        UpdateStatus();
    }
    private void ToggleStartup()
    {
        try
        {
            if (startupStore is RegistryStartupStore registry && registry.DisabledByWindows)
            {
                if (MessageBox.Show("自動起動はWindows設定で無効になっています。設定を開いて切り替えますか？", "自動起動", MessageBoxButtons.YesNo, MessageBoxIcon.Information) == DialogResult.Yes)
                    Process.Start(new ProcessStartInfo("ms-settings:startupapps") { UseShellExecute = true });
                return;
            }
            startup.SetEnabled(!startup.Enabled);
            log.Write("startup-changed");
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.Security.SecurityException or System.ComponentModel.Win32Exception)
        { log.Write("startup-change-failed"); ShowBalloon("自動起動設定を変更できませんでした。", ToolTipIcon.Error); }
    }
    private void EditDelay()
    {
        using var form = new Form { Text = "コピー待機時間", FormBorderStyle = FormBorderStyle.FixedDialog, StartPosition = FormStartPosition.CenterScreen, ClientSize = new Size(390, 155), MaximizeBox = false, MinimizeBox = false };
        var label = new Label { Text = "新しい文字起こしの検出後に待つ秒数（0～60秒）", AutoSize = true, Location = new Point(16, 18) };
        var number = new NumericUpDown { Minimum = 0, Maximum = 60, DecimalPlaces = 1, Increment = 0.5m, Value = (decimal)copies.Delay.TotalSeconds, Location = new Point(18, 52), Width = 120 };
        var ok = new Button { Text = "保存", DialogResult = DialogResult.OK, Location = new Point(200, 105) };
        var cancel = new Button { Text = "キャンセル", DialogResult = DialogResult.Cancel, Location = new Point(285, 105) };
        form.Controls.AddRange([label, number, ok, cancel]); form.AcceptButton = ok; form.CancelButton = cancel;
        if (form.ShowDialog() != DialogResult.OK) return;
        copyGeneration++; copies.Cancel();
        copies.Delay = TimeSpan.FromSeconds((double)number.Value);
        settings = settings with { DelaySeconds = (double)number.Value }; SaveSettings();
    }
    private void SaveSettings()
    {
        try { settings.Save(paths); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { log.Write("settings-save-failed"); ShowBalloon("設定を保存できませんでした。この起動中のみ有効です。", ToolTipIcon.Warning); }
    }
    private void ShowLog()
    {
        using var form = new Form { Text = "Codex Voice Clipboard — ログ（本文は記録しません）", Width = 780, Height = 500, StartPosition = FormStartPosition.CenterScreen };
        var text = new TextBox { Multiline = true, ReadOnly = true, ScrollBars = ScrollBars.Both, WordWrap = false, Dock = DockStyle.Fill };
        try { text.Text = File.Exists(paths.Log) ? File.ReadAllText(paths.Log) : "ログはありません。"; }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { text.Text = "ログを読み取れません。"; }
        form.Controls.Add(text); form.ShowDialog();
    }
    private static void ShowRisk() => MessageBox.Show(
        "このアプリは新しい文字起こしを検出してから、設定秒数後にクリップボードへコピーします。\n\n待機中に別の内容をコピーすると、文字起こしで上書きされる場合があります。必要に応じて自動コピーをOFFにし、手動コピーを使ってください。\n\nコピー後に別の内容をコピーした場合、このアプリは同じ文字起こしを再コピーしません。本文はログに残さず、ネットワーク送信もしません。",
        "クリップボード上書きについて", MessageBoxButtons.OK, MessageBoxIcon.Information);
    private void StartUninstall()
    {
        try { Installation.StartUninstall(paths); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        { log.Write("uninstall-launch-failed"); ShowBalloon("アンインストールを開始できませんでした。", ToolTipIcon.Error); }
    }
    private void ShowBalloon(string message, ToolTipIcon icon) { if (paths.Sandbox is null) tray.ShowBalloonTip(4000, "Codex Voice Clipboard", message, icon); }
    private void UpdateStatus()
    {
        string nextState = historyError || copies.HasError || watcherError ? "error" : copies.Enabled ? "active" : "paused";
        string label = historyError ? "エラー: 履歴を読み取れません" : copies.HasError ? "エラー: コピー失敗（手動で再試行可）" : watcherError ? "エラー: 通知監視失敗（定期確認中）" : copies.Enabled ? "監視中: 自動コピー ON" : "一時停止中: 手動コピー可能";
        statusItem.Text = label;
        if (iconState != nextState)
        {
            iconState = nextState;
            tray.Icon = nextState switch { "active" => icons.Active, "paused" => icons.Paused, _ => icons.Error };
            log.Write("state-" + nextState);
        }
        tray.Text = "Codex Voice Clipboard: " + label;
        if (paths.Sandbox is not null)
        {
            try { paths.WriteJson("state.json", new { pid = Environment.ProcessId, sta = Thread.CurrentThread.GetApartmentState() == ApartmentState.STA, recentCount = recent.Count, historyError, watcherHealthy = watcher is not null && !watcherError, autoEnabled = copies.Enabled, pending = copies.HasPending, clipboardError = copies.HasError, state = nextState }); }
            catch (IOException) { }
        }
    }
    private async Task ConsumeSandboxCommand()
    {
        var path = Path.Combine(paths.StateDirectory, "command.txt");
        if (!File.Exists(path)) return;
        try
        {
            string command = File.ReadAllText(path).Trim();
            switch (command)
            {
                case "pause": await SetAuto(false); break;
                case "resume": await SetAuto(true); break;
                case "manual": if (recent.Count > 0) ManualCopy(recent[0]); break;
                case "startup-on": startup.SetEnabled(true); break;
                case "startup-off": startup.SetEnabled(false); break;
                case "uninstall": StartUninstall(); break;
                case "exit": ExitThread(); break;
            }
            File.Delete(path);
        }
        catch (IOException) { }
    }
    protected override void ExitThreadCore()
    {
        if (closing) return;
        closing = true; copies.Cancel(); timer.Stop(); timer.Dispose();
        watcher?.Dispose(); watcher = null;
        tray.Visible = false; tray.Dispose(); menu.Dispose();
        stopEvent.Dispose(); dispatcher.Dispose();
        if (!scanning && !resuming) history.Dispose();
        icons.Dispose();
        log.Write("application-stopped");
        base.ExitThreadCore();
    }
}
