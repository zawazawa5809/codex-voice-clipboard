using System.Runtime.InteropServices;
using CodexVoiceClipboard.Core;
using Microsoft.Win32;

namespace CodexVoiceClipboard;

internal sealed class WindowsClipboard : IClipboard
{
    public void SetText(string text)
    {
        if (Thread.CurrentThread.GetApartmentState() != ApartmentState.STA) throw new ThreadStateException("STA required.");
        Clipboard.SetText(text, TextDataFormat.UnicodeText);
    }
}

internal sealed class SandboxClipboard(AppPaths paths) : IClipboard
{
    public void SetText(string text)
    {
        if (Thread.CurrentThread.GetApartmentState() != ApartmentState.STA) throw new ThreadStateException("STA required.");
        var failuresPath = Path.Combine(paths.StateDirectory, "clipboard-failures.txt");
        if (File.Exists(failuresPath) && int.TryParse(File.ReadAllText(failuresPath), out int failures) && failures > 0)
        {
            File.WriteAllText(failuresPath, (failures - 1).ToString(System.Globalization.CultureInfo.InvariantCulture));
            throw new ExternalException("Synthetic clipboard contention.");
        }
        File.WriteAllText(Path.Combine(paths.StateDirectory, "clipboard.txt"), text);
    }
}

internal sealed class RegistryStartupStore : IStartupStore
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ApprovedKey = @"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    private const string ValueName = "CodexVoiceClipboard";
    public string? Command { get { using var key = Registry.CurrentUser.OpenSubKey(RunKey); return key?.GetValue(ValueName) as string; } }
    public bool DisabledByWindows
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(ApprovedKey);
            // Windows owns this value. Never rewrite it to override a Settings/Task Manager choice.
            return key?.GetValue(ValueName) is byte[] bytes && bytes.Length > 0 && (bytes[0] == 3 || bytes[0] == 7);
        }
    }
    public void Write(string? command)
    {
        using var key = Registry.CurrentUser.CreateSubKey(RunKey);
        if (command is null) key.DeleteValue(ValueName, false);
        else key.SetValue(ValueName, command, RegistryValueKind.String);
    }
    public void RemoveApproval()
    {
        using var key = Registry.CurrentUser.OpenSubKey(ApprovedKey, writable: true);
        key?.DeleteValue(ValueName, false);
    }
}

internal sealed class SandboxStartupStore(AppPaths paths) : IStartupStore
{
    private string PathName => Path.Combine(paths.StateDirectory, "startup.txt");
    public string? Command => File.Exists(PathName) ? File.ReadAllText(PathName) : null;
    public void Write(string? command) { if (command is null) File.Delete(PathName); else File.WriteAllText(PathName, command); }
}

internal sealed class SingleInstance : IDisposable
{
    private readonly Mutex mutex;
    public bool Acquired { get; }
    public SingleInstance(string name)
    {
        mutex = new Mutex(false, name);
        try { Acquired = mutex.WaitOne(0); } catch (AbandonedMutexException) { Acquired = true; }
    }
    public void Dispose() { if (Acquired) mutex.ReleaseMutex(); mutex.Dispose(); }
}
