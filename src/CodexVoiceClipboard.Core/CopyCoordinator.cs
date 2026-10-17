using System.Runtime.InteropServices;

namespace CodexVoiceClipboard.Core;

public interface IClipboard { void SetText(string text); }

/// <summary>Called only on the UI STA thread, including Tick. Stores at most one pending copy.</summary>
public sealed class CopyCoordinator(IClipboard clipboard)
{
    private bool enabled = true;
    private Transcript? pending;
    private DateTimeOffset due;
    private int attempts;
    public TimeSpan Delay { get; set; } = TimeSpan.FromSeconds(2);
    public bool HasError { get; private set; }
    public bool HasPending => pending is not null;
    public event Action<string>? Event;
    public bool Enabled
    {
        get => enabled;
        set { enabled = value; Cancel(); }
    }
    public void Cancel() { pending = null; attempts = 0; HasError = false; }
    public void Observe(IReadOnlyList<Transcript> records, DateTimeOffset now)
    {
        if (enabled && records.Count > 0) Schedule(records[^1], now + Delay);
    }
    public void CopyManually(Transcript record, DateTimeOffset now) => Schedule(record, now);
    private void Schedule(Transcript record, DateTimeOffset when) { pending = record; due = when; attempts = 0; HasError = false; }
    public void Tick(DateTimeOffset now)
    {
        if (pending is null || now < due) return;
        attempts++;
        try { clipboard.SetText(pending.Text); pending = null; HasError = false; Event?.Invoke("clipboard-copied"); }
        catch (Exception ex) when (ex is ExternalException or ThreadStateException)
        {
            if (attempts >= 8) { pending = null; HasError = true; Event?.Invoke("clipboard-failed"); }
            else { due = now.AddMilliseconds(250); Event?.Invoke("clipboard-retry"); }
        }
    }
}
