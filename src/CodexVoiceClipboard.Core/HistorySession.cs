using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace CodexVoiceClipboard.Core;

public sealed record Transcript(string Id, string Text);
public sealed record HistoryBatch(IReadOnlyList<Transcript> NewRecords, int InvalidLines);

/// <summary>Single-consumer incremental reader. Never opens the source for writing.</summary>
public sealed class HistorySession(string path) : IDisposable
{
    private readonly HashSet<string> seen = new(StringComparer.Ordinal);
    private readonly List<Transcript> recent = [];
    private readonly MemoryStream fragment = new();
    private long offset;
    private string? identity;
    private bool baselineDone;
    private bool discardUntilNewline;
    private byte[] checkpoint = [];
    public IReadOnlyList<Transcript> Recent => recent;

    public HistoryBatch Poll()
    {
        FileStream file;
        try { file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete); }
        catch (FileNotFoundException) { baselineDone = true; Reset(); throw; }
        catch (DirectoryNotFoundException) { baselineDone = true; Reset(); throw; }
        using (file)
        {
            var currentIdentity = FileIdentity(file, path);
            if (identity != currentIdentity || file.Length < offset || !CheckpointMatches(file)) Reset();
            identity = currentIdentity;
            file.Position = offset;
            List<Transcript> records = [];
            int invalid = 0;
            var buffer = new byte[64 * 1024];
            // Capture a finite boundary even while the writer is appending.
            long remaining = file.Length - offset;
            while (remaining > 0)
            {
                int count = file.Read(buffer, 0, (int)Math.Min(buffer.Length, remaining));
                if (count == 0) break;
                offset += count; remaining -= count;
                for (int i = 0; i < count; i++)
                {
                    byte value = buffer[i];
                    if (value == (byte)'\n')
                    {
                        if (!discardUntilNewline) ParseLine(records, ref invalid, complete: true);
                        fragment.SetLength(0); discardUntilNewline = false;
                    }
                    else if (!discardUntilNewline)
                    {
                        fragment.WriteByte(value);
                        // Bounded malformed input. A valid transcription can still be up to 16 MiB.
                        if (fragment.Length > 16 * 1024 * 1024) { fragment.SetLength(0); discardUntilNewline = true; invalid++; }
                    }
                }
            }
            // A complete final JSON object is accepted even without its final newline.
            // Incomplete UTF8/JSON remains buffered for the next poll.
            if (fragment.Length > 0 && ParseLine(records, ref invalid, complete: false)) fragment.SetLength(0);
            int checkpointSize = (int)Math.Min(64, offset);
            checkpoint = new byte[checkpointSize];
            file.Position = offset - checkpointSize;
            file.ReadExactly(checkpoint);
            bool suppress = !baselineDone;
            baselineDone = true;
            return new(suppress ? [] : records, invalid);
        }
    }

    private bool ParseLine(List<Transcript> records, ref int invalid, bool complete)
    {
        var bytes = fragment.ToArray().AsSpan();
        // BOM only matters at the first line; allowing it here also handles producer rewrites.
        if (bytes.StartsWith(new byte[] { 0xef, 0xbb, 0xbf })) bytes = bytes[3..];
        if (bytes.IsEmpty || Encoding.UTF8.GetString(bytes).Trim().Length == 0) return true;
        try
        {
            using var document = JsonDocument.Parse(bytes.ToArray());
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object ||
                !root.TryGetProperty("id", out var idValue) || idValue.ValueKind != JsonValueKind.String ||
                !root.TryGetProperty("text", out var textValue) || textValue.ValueKind != JsonValueKind.String ||
                string.IsNullOrWhiteSpace(idValue.GetString()) || string.IsNullOrWhiteSpace(textValue.GetString()))
            { if (complete) invalid++; return complete; }
            var transcript = new Transcript(idValue.GetString()!, textValue.GetString()!);
            if (seen.Add(transcript.Id))
            {
                recent.Insert(0, transcript);
                if (recent.Count > 10) recent.RemoveAt(10);
                if (baselineDone) records.Add(transcript);
            }
            return true;
        }
        catch (JsonException) { if (complete) invalid++; return false; }
    }

    private bool CheckpointMatches(FileStream file)
    {
        if (checkpoint.Length == 0) return true;
        file.Position = offset - checkpoint.Length;
        var current = new byte[checkpoint.Length];
        return file.Read(current) == current.Length && current.AsSpan().SequenceEqual(checkpoint);
    }
    private void Reset() { offset = 0; fragment.SetLength(0); identity = null; discardUntilNewline = false; checkpoint = []; }
    public void Dispose() => fragment.Dispose();

    private static string FileIdentity(FileStream stream, string filePath)
    {
        if (OperatingSystem.IsWindows())
        {
            if (!GetFileInformationByHandle(stream.SafeFileHandle, out var info))
                throw new IOException("Cannot identify history file.", Marshal.GetExceptionForHR(Marshal.GetHRForLastWin32Error()));
            return $"{info.VolumeSerialNumber}:{info.FileIndexHigh}:{info.FileIndexLow}";
        }
        return File.GetCreationTimeUtc(filePath).Ticks.ToString(System.Globalization.CultureInfo.InvariantCulture);
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct FileInfoNative
    {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME CreationTime, LastAccessTime, LastWriteTime;
        public uint VolumeSerialNumber, FileSizeHigh, FileSizeLow, NumberOfLinks, FileIndexHigh, FileIndexLow;
    }
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInfoNative info);
}
