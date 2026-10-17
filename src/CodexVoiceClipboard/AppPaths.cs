using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;

namespace CodexVoiceClipboard;

internal sealed class AppPaths
{
    public string? Sandbox { get; }
    public string StateDirectory { get; }
    public string History { get; }
    public string InstallDirectory { get; }
    public string InstalledExe => Path.Combine(InstallDirectory, "CodexVoiceClipboard.exe");
    public string Log => Path.Combine(StateDirectory, "app.log");
    public string MutexName { get; }
    public string StopName => MutexName + ".Stop";
    public AppPaths(string? sandbox)
    {
        Sandbox = sandbox is null ? null : Path.GetFullPath(sandbox);
        StateDirectory = Sandbox ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CodexVoiceClipboard");
        InstallDirectory = Sandbox is null
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "CodexVoiceClipboard")
            : Path.Combine(Sandbox, "install");
        History = Sandbox is null
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".codex", "transcription-history.jsonl")
            : Path.Combine(Sandbox, "history.jsonl");
        var identity = Sandbox is null ? WindowsIdentity.GetCurrent().User!.Value : Sandbox.ToUpperInvariant();
        MutexName = "Local\\CodexVoiceClipboard." + Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(identity)))[..24];
    }
    public void WriteJson(string filename, object value)
    {
        Directory.CreateDirectory(StateDirectory);
        var path = Path.Combine(StateDirectory, filename);
        var temp = path + ".tmp";
        File.WriteAllText(temp, JsonSerializer.Serialize(value));
        File.Move(temp, path, true);
    }
}

internal sealed record AppSettings(bool AutoCopy = true, double DelaySeconds = 2)
{
    public static AppSettings Load(AppPaths paths)
    {
        try
        {
            var path = Path.Combine(paths.StateDirectory, "settings.json");
            if (!File.Exists(path)) return new();
            var settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(path)) ?? new();
            return settings with { DelaySeconds = double.IsFinite(settings.DelaySeconds) ? Math.Clamp(settings.DelaySeconds, 0, 60) : 2 };
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException) { return new(); }
    }
    public void Save(AppPaths paths) => paths.WriteJson("settings.json", this);
}
