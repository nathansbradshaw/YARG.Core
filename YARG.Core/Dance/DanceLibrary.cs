using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using YARG.Core.Chart;
using YARG.Core.Logging;
using YARG.Core.Song;

namespace YARG.Core.Dance
{
    public static class DanceLibrary
    {
        // This pass intentionally runs even after a quick cache scan. Simfiles are
        // independent inputs, and must not inherit the cached band-chart timestamp.
        public static void Refresh(SongCache cache, IEnumerable<string> roots)
        {
            var entries = cache.Entries.Values.SelectMany(e => e).Where(e => e is not DanceSongEntry).ToList();
            var directories = new HashSet<string>(StringComparer.Ordinal);
            var consumed = new HashSet<string>(StringComparer.Ordinal);
            foreach (var entry in entries)
            {
                if (!Directory.Exists(entry.ActualLocation)) continue;
                directories.Add(Path.GetFullPath(entry.ActualLocation));
                TryAttach(entry, consumed);
            }

            var visited = new HashSet<string>(StringComparer.Ordinal);
            foreach (var root in roots.Where(Directory.Exists))
            {
                var pending = new Stack<string>(); pending.Push(Path.GetFullPath(root));
                while (pending.Count > 0)
                {
                    var dir = pending.Pop();
                    if (!visited.Add(dir) || consumed.Contains(dir)) continue;
                    try
                    {
                        if (!directories.Contains(dir))
                        {
                            var parts = Discover(dir);
                            if (parts != null)
                            {
                                var entry = new DanceSongEntry(dir, parts);
                                entries.Add(entry); directories.Add(dir);
                                Consume(parts, consumed);
                            }
                        }
                        foreach (var subdir in Directory.EnumerateDirectories(dir).OrderByDescending(p => p, StringComparer.Ordinal))
                        {
                            var name = Path.GetFileName(subdir);
                            if (name.StartsWith(".") || name == "yace-history" ||
                                (File.GetAttributes(subdir) & FileAttributes.ReparsePoint) != 0) continue;
                            pending.Push(subdir);
                        }
                    }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or FormatException)
                    { YargLogger.LogException(ex, $"Could not scan Dance folder {dir}"); }
                }
            }
            cache.Entries.Clear();
            foreach (var entry in entries)
            {
                if (!cache.Entries.TryGetValue(entry.Hash, out var group)) cache.Entries[entry.Hash] = group = new();
                group.Add(entry);
            }
        }

        private static void TryAttach(SongEntry entry, HashSet<string> consumed)
        {
            if (entry.DanceBandHash.HasValue) entry.SetDanceHash(entry.DanceBandHash.Value);
            entry.DanceParts = null;
            try
            {
                var parts = Discover(entry.ActualLocation);
                if (parts == null) return;
                entry.DanceBandHash ??= entry.Hash;
                entry.DanceParts = parts;
                entry.SetDanceHash(CombinedHash(parts, entry.Hash.HashBytes, entry.SongOffsetSeconds));
                Consume(parts, consumed);
            }
            catch (Exception ex) { YargLogger.LogException(ex, $"Could not attach Dance charts to {entry}"); }
        }

        private static void Consume(DanceParts parts, HashSet<string> consumed)
        {
            foreach (var song in new[] { parts.Pad, parts.Keyboard }.Where(s => s != null))
                consumed.Add(Path.GetDirectoryName(song!.Source)!);
        }

        public static DanceParts? Discover(string directory)
        {
            var pad = PreferredFile(Path.Combine(directory, "YACE-pad"));
            var keyboard = PreferredFile(Path.Combine(directory, "YACE-keyboard"));
            var root = PreferredFile(directory);
            if (pad == null && keyboard == null && root == null) return null;
            var parts = new DanceParts();
            if (pad != null) parts.Pad = DanceSongData.Load(pad);
            if (keyboard != null) parts.Keyboard = DanceSongData.Load(keyboard);
            if (root != null)
            {
                var song = DanceSongData.Load(root);
                parts.Pad ??= song; parts.Keyboard ??= song;
            }
            return parts;
        }

        private static string? PreferredFile(string directory)
        {
            if (!Directory.Exists(directory)) return null;
            var files = Directory.EnumerateFiles(directory).OrderBy(p => p, StringComparer.Ordinal).ToArray();
            var ssc = files.Where(p => Path.GetExtension(p).Equals(".ssc", StringComparison.OrdinalIgnoreCase)).ToArray();
            var candidates = ssc.Length > 0 ? ssc : files.Where(p => Path.GetExtension(p).Equals(".sm", StringComparison.OrdinalIgnoreCase)).ToArray();
            if (candidates.Length > 1) throw new FormatException("Keep one simfile per song directory; SSC takes precedence over SM");
            return candidates.FirstOrDefault();
        }

        internal static HashWrapper CombinedHash(DanceParts parts, byte[]? bandHash = null, double clockOffset = 0)
        {
            using var content = new MemoryStream();
            if (bandHash != null) content.Write(bandHash, 0, bandHash.Length);
            var offset = BitConverter.GetBytes(clockOffset);
            content.Write(offset, 0, offset.Length);
            foreach (var song in new[] { parts.Pad, parts.Keyboard })
            {
                var bytes = Encoding.UTF8.GetBytes(song?.SourceText ?? "");
                var length = BitConverter.GetBytes(bytes.Length);
                content.Write(length, 0, length.Length); content.Write(bytes, 0, bytes.Length);
            }
            return HashWrapper.Hash(content.ToArray());
        }

        public static void AttachToChart(SongEntry entry, SongChart chart)
        {
            entry.DanceParts?.ValidateSources();
            var parts = entry.DanceParts;
            if (parts == null || entry.SongOffsetSeconds == 0) { chart.DanceParts = parts; return; }
            DanceSongData? Shift(DanceSongData? song) => song == null ? null :
                DanceNative.Parse(song.SourceText, Path.GetExtension(song.Source).Equals(".ssc", StringComparison.OrdinalIgnoreCase), entry.SongOffsetSeconds);
            chart.DanceParts = new DanceParts { Pad = Shift(parts.Pad), Keyboard = Shift(parts.Keyboard) };
        }
    }
}
