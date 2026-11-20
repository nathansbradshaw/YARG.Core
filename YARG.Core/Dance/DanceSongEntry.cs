using System;
using System.IO;
using System.Linq;
using YARG.Core.Audio;
using YARG.Core.Chart;
using YARG.Core.IO;
using YARG.Core.Song;

namespace YARG.Core.Dance
{
    // Transient entry: rescanned from simfiles instead of extending the binary
    // band-song cache layout. It still participates in normal hashes and sorting.
    public sealed class DanceSongEntry : SongEntry
    {
        private readonly string _directory;
        public override EntryType SubType => EntryType.Ini;
        public override string SortBasedLocation => _directory;
        public override string ActualLocation => _directory;

        internal DanceSongEntry(string directory, DanceParts parts)
        {
            _directory = directory;
            DanceParts = parts;
            var song = parts.Pad ?? parts.Keyboard!;
            if (song.Asset(song.Music) == null) throw new IOException("Simfile MUSIC is missing or outside its song directory");
            _metadata.Name = string.IsNullOrWhiteSpace(song.Title) ? Path.GetFileName(directory) : song.Title;
            _metadata.Artist = string.IsNullOrWhiteSpace(song.Artist) ? SongMetadata.DEFAULT_ARTIST : song.Artist;
            _metadata.Source = "StepMania";
            _metadata.Playlist = Path.GetFileName(Path.GetDirectoryName(directory)) ?? "StepMania";
            _metadata.SongLength = (long)Math.Ceiling(parts.Charts.Max(c => c.EndTime) * 1000);
            _name = new SortString(_metadata.Name);
            _artist = new SortString(_metadata.Artist);
            _album = new SortString(_metadata.Album);
            _source = new SortString(_metadata.Source);
            _playlist = new SortString(_metadata.Playlist);
            _hash = DanceLibrary.CombinedHash(parts);
        }

        public override DateTime GetLastWriteTime() => new[] { DanceParts!.Pad, DanceParts.Keyboard }
            .Where(s => s != null).Max(s => File.GetLastWriteTime(s!.Source));
        public override SongChart LoadChart()
        {
            DanceParts!.ValidateSources();
            var chart = new SongChart(480) { DanceParts = DanceParts };
            chart.SyncTrack.Tempos.Add(new TempoChange(120, 0, 0));
            chart.SyncTrack.TimeSignatures.Add(new TimeSignatureChange(4, 4, 0, 0, 0, 0, 0, 0));
            chart.SyncTrack.FinishLoading((uint)Math.Max(1920, DanceParts.Charts.Max(c => c.EndTime) * 960));
            return chart;
        }
        public override StemMixer? LoadAudio(float speed, double volume, params SongStem[] ignoreStems)
        {
            var song = DanceParts!.Pad ?? DanceParts.Keyboard!;
            var file = song.Asset(song.Music);
            return file == null ? null : GlobalAudioHandler.LoadCustomFile(file, speed, volume, true, SongStem.Song);
        }
        public override StemMixer? LoadPreviewAudio(float speed) => LoadAudio(speed, 0);
        public override YARGImage? LoadAlbumData()
        {
            var song = DanceParts!.Pad ?? DanceParts.Keyboard!;
            var file = song.Asset(song.Banner);
            return file == null ? null : YARGImage.Load(file);
        }
        public override BackgroundResult? LoadBackground(bool excludeYarground = false) => null;
        public override FixedArray<byte>? LoadMiloData() => null;
    }
}
