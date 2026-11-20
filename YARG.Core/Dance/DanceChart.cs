using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using YARG.Core.Song;

namespace YARG.Core.Dance
{
    public sealed class DanceNote
    {
        [JsonProperty("lane")] public byte Lane;
        [JsonProperty("kind")] public string Kind = "tap";
        [JsonProperty("beat")] public double Beat;
        [JsonProperty("end_beat")] public double EndBeat;
        [JsonProperty("time")] public double Time;
        [JsonProperty("end_time")] public double EndTime;
        [JsonProperty("judgable")] public bool Judgable;
        [JsonIgnore] public bool IsSustain => Kind is "hold" or "roll";
    }

    public sealed class DanceChart
    {
        [JsonProperty("name")] public string Name = "";
        [JsonProperty("difficulty")] public Difficulty Difficulty;
        [JsonProperty("meter")] public int Meter;
        [JsonProperty("timing")] public JObject Timing = new();
        [JsonProperty("notes")] public DanceNote[] Notes = Array.Empty<DanceNote>();
        [JsonIgnore] public DancePowerPhrase[] PowerPhrases = Array.Empty<DancePowerPhrase>();
        [JsonIgnore] public int TotalNotes => Notes.Count(n => n.Judgable && n.Kind != "mine");
        [JsonIgnore] public double FirstTime => Notes.Length == 0 ? 0 : Notes.Min(n => n.Time);
        [JsonIgnore] public double EndTime => Notes.Length == 0 ? 0 : Notes.Max(n => n.EndTime);
    }

    public sealed class DanceSongData
    {
        [JsonProperty("title")] public string Title = "";
        [JsonProperty("artist")] public string Artist = "";
        [JsonProperty("music")] public string Music = "";
        [JsonProperty("banner")] public string Banner = "";
        [JsonProperty("charts")] public DanceChart[] Charts = Array.Empty<DanceChart>();
        [JsonIgnore] public string Source = "";
        [JsonIgnore] public string SourceText = "";

        public static DanceSongData Load(string file)
        {
            if (new FileInfo(file).Length > 32 * 1024 * 1024) throw new FormatException("Simfile exceeds 32 MiB");
            var text = File.ReadAllText(file);
            var result = DanceNative.Parse(text, Path.GetExtension(file).Equals(".ssc", StringComparison.OrdinalIgnoreCase));
            result.Source = file;
            result.SourceText = text;
            return result;
        }

        public string? Asset(string relative)
        {
            if (string.IsNullOrWhiteSpace(relative) || Path.IsPathRooted(relative)) return null;
            var root = Path.GetDirectoryName(Source)! + Path.DirectorySeparatorChar;
            var path = Path.GetFullPath(Path.Combine(root, relative.Replace('\\', Path.DirectorySeparatorChar)));
            return path.StartsWith(root, StringComparison.Ordinal) && File.Exists(path) ? path : null;
        }
    }

    public sealed class DanceParts
    {
        public DanceSongData? Pad { get; internal set; }
        public DanceSongData? Keyboard { get; internal set; }
        public IEnumerable<DanceChart> Charts => new[] { Pad, Keyboard }.Where(s => s != null).SelectMany(s => s!.Charts);
        public DanceSongData? For(Instrument instrument) => instrument == Instrument.Dance ? Pad : Keyboard;
        public DanceChart GetChart(Instrument instrument, Difficulty difficulty) =>
            For(instrument)?.Charts.FirstOrDefault(c => c.Difficulty == difficulty)
            ?? throw new InvalidOperationException($"No {instrument} chart for {difficulty}; rescan songs");
        public PartValues GetPart(Instrument instrument)
        {
            var part = PartValues.Default;
            var song = For(instrument);
            if (song == null) return part;
            foreach (var chart in song.Charts)
            {
                part.Difficulties |= chart.Difficulty.ToDifficultyMask();
                part.Intensity = Math.Max(part.Intensity, (sbyte)Math.Clamp(chart.Meter / 3, 0, 6));
            }
            return part;
        }
        public void ValidateSources()
        {
            foreach (var song in new[] { Pad, Keyboard }.Where(s => s != null).Distinct())
                if (File.ReadAllText(song!.Source) != song.SourceText)
                    throw new IOException("A Dance simfile changed; rescan songs before playing");
        }
    }
}
