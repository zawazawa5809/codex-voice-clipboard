using Microsoft.Win32;

namespace CodexVoiceClipboard;

/// <summary>Embedded, multi-resolution A-design icons; no fonts or generated HICONs.</summary>
internal sealed class TrayIcons : IDisposable
{
    public Icon Active { get; }
    public Icon Paused { get; }
    public Icon Error { get; }

    public TrayIcons(bool lightTaskbar)
    {
        string theme = lightTaskbar ? "light" : "dark";
        Active = Load("active-" + theme);
        try
        {
            Paused = Load("paused-" + theme);
            try { Error = Load("error-" + theme); }
            catch { Paused.Dispose(); throw; }
        }
        catch { Active.Dispose(); throw; }
    }

    private static Icon Load(string name)
    {
        using var stream = typeof(TrayIcons).Assembly.GetManifestResourceStream($"CodexVoiceClipboard.Icons.{name}.ico")
            ?? throw new InvalidOperationException("Embedded icon is missing.");
        using var icon = new Icon(stream, SystemInformation.SmallIconSize);
        return (Icon)icon.Clone();
    }

    public static bool DetectLightTaskbar()
    {
        if (SystemInformation.HighContrast) return SystemColors.WindowText.GetBrightness() < 0.5f;
        try
        {
            // Read only: Windows taskbar theme is never changed by this application.
            using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            return key?.GetValue("SystemUsesLightTheme") is int value && value != 0;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.Security.SecurityException) { return false; }
    }

    public void Dispose() { Active.Dispose(); Paused.Dispose(); Error.Dispose(); }
}
