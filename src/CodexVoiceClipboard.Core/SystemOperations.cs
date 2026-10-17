using System.Text.RegularExpressions;

namespace CodexVoiceClipboard.Core;

public interface IStartupStore { string? Command { get; } void Write(string? command); }
public sealed class StartupController(IStartupStore store, string executable)
{
    public bool Enabled => store.Command is not null;
    public void SetEnabled(bool enabled) => store.Write(enabled ? $"\"{executable}\"" : null);
}

public interface IUninstallOperations
{
    bool StopResident();
    void DisableStartup();
    void RemoveRegistration();
    void DeleteApplication();
}
public sealed class UninstallCoordinator(IUninstallOperations operations)
{
    public void Execute()
    {
        if (!operations.StopResident()) throw new IOException("Resident did not stop. Removal cancelled.");
        operations.DisableStartup();
        operations.RemoveRegistration();
        operations.DeleteApplication();
    }
}

/// <summary>Only fixed event codes are permitted. No exception messages or transcript data.</summary>
public sealed partial class SafeLog(string path)
{
    private readonly object sync = new();
    public string Path { get; } = path;
    [GeneratedRegex("^[a-z]+(?:-[a-z]+)*$", RegexOptions.CultureInvariant)]
    private static partial Regex EventCode();
    public void Write(string code)
    {
        if (!EventCode().IsMatch(code)) throw new ArgumentException("Fixed event code required.", nameof(code));
        try
        {
            lock (sync)
            {
                Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path)!);
                if (File.Exists(Path) && new FileInfo(Path).Length > 512 * 1024) File.Move(Path, Path + ".1", true);
                File.AppendAllText(Path, $"{DateTimeOffset.Now:O} {code}{Environment.NewLine}");
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { /* Logging cannot stop recovery. */ }
    }
}
