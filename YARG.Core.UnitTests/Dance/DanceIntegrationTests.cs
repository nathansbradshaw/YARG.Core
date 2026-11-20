using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using NUnit.Framework;
using YARG.Core.Chart;
using YARG.Core.Dance;
using YARG.Core.Engine;
using YARG.Core.Engine.Drums;
using YARG.Core.Engine.Drums.Engines;
using YARG.Core.Engine.Guitar.Engines;
using YARG.Core.Engine.Vocals.Engines;
using YARG.Core.Game;
using YARG.Core.Input;
using YARG.Core.IO;
using YARG.Core.Replays;
using YARG.Core.Song;
using YARG.Core.Song.Cache;

namespace YARG.Core.UnitTests.Dance;

// These verify the real Rust ABI, normal MIDI scan, all five engines, and replay
// serialization together. Rust unit tests cover individual judgment rules.
public class DanceIntegrationTests
{
    private string _temporary = null!;
    private string _song = null!;
    private static string CoreRoot([CallerFilePath] string source = "") =>
        Path.GetFullPath(Path.Combine(Path.GetDirectoryName(source)!, "../.."));

    [OneTimeSetUp]
    public void GenerateYaceFixture()
    {
        _temporary = Path.Combine(Path.GetTempPath(), "yarg-dance-tests-" + Guid.NewGuid().ToString("N"));
        _song = Path.Combine(_temporary, "mixed");
        var start = new ProcessStartInfo("cargo") { WorkingDirectory = CoreRoot(), UseShellExecute = false };
        foreach (var arg in new[] { "run", "--locked", "--offline", "--manifest-path", "Native/dance/Cargo.toml", "--bin", "fixture", "--", _song }) start.ArgumentList.Add(arg);
        using var process = Process.Start(start)!;
        process.WaitForExit();
        Assert.That(process.ExitCode, Is.Zero, "Rust fixture generation failed");
        Assert.That(DanceNative.yarg_dance_abi(), Is.EqualTo(1));
    }

    [OneTimeTearDown]
    public void Cleanup() { if (Directory.Exists(_temporary)) Directory.Delete(_temporary, true); }

    private SongCache Scan(bool quick = false)
    {
        var cache = CacheHandler.RunScan(quick, Path.Combine(_temporary, "cache.bin"),
            Path.Combine(_temporary, "bad.txt"), false, new List<string> { _song });
        DanceLibrary.Refresh(cache, new[] { _song });
        return cache;
    }
    private static SongEntry Only(SongCache cache) => cache.Entries.Values.SelectMany(e => e).Single();

    [Test]
    public void DanceScoresMatchGuitarForTheSameUnpoweredSingleNotes()
    {
        var chart = Only(Scan()).LoadChart()!;
        var source = chart.FiveFretGuitar.GetDifficulty(Difficulty.Expert).Clone();
        var thresholds = new[] { .06f, .12f, .20f, .47f, .78f, 1.15f };
        var guitar = new YargFiveFretGuitarEngine(source, chart.SyncTrack,
            EnginePreset.Default.FiveFretGuitar.Create(thresholds, new float[6], false), true);
        var notes = new DanceChart { Timing = chart.DanceParts!.GetChart(Instrument.Dance, Difficulty.Expert).Timing, Notes = source.Notes.Select((n, i) => new DanceNote {
            Lane = (byte)(i % 4), Beat = n.Tick / (double)chart.Resolution, EndBeat = n.Tick / (double)chart.Resolution,
            Time = n.Time, EndTime = n.Time, Kind = "tap", Judgable = true }).ToArray() };
        using var dance = new DanceEngine(notes, chart.SyncTrack, true);
        guitar.AllowStarPower(false); dance.AllowStarPower(false);
        guitar.Update(14); dance.Update(14);
        Assert.That(dance.Stats.TotalScore, Is.EqualTo(guitar.BaseStats.TotalScore));
        Assert.That(dance.Stats.NoteScore, Is.EqualTo(guitar.BaseStats.NoteScore));
        Assert.That(dance.Stats.ScoreMultiplier, Is.EqualTo(guitar.BaseStats.ScoreMultiplier));
        Assert.That(dance.BaseScore, Is.EqualTo(guitar.BaseScore));
        Assert.That(dance.BaseNoteScore, Is.EqualTo(guitar.BaseNoteScore));
        Assert.That(dance.Stats.StarPowerPhrasesHit, Is.Zero);
        Assert.That(Marshal.SizeOf<DanceScoreState>(), Is.EqualTo(88));
    }

    [Test]
    public void ManualStarPowerEarnsDoublesDrainsAndSurvivesReplaySeeking()
    {
        var chart = Only(Scan()).LoadChart()!;
        var notes = chart.DanceParts!.GetChart(Instrument.FourKey, Difficulty.Expert);
        Assert.That(notes.PowerPhrases.Length, Is.EqualTo(2));
        using var engine = new DanceEngine(notes, chart.SyncTrack, false);
        var inputs = new List<GameInput>();
        foreach (var note in notes.Notes.Where(n => n.Judgable && n.Kind != "mine"))
        {
            inputs.Add(new GameInput(note.Time, note.Lane, true));
            inputs.Add(new GameInput(Math.Max(note.Time + .01, note.EndTime), note.Lane, false));
        }
        inputs.Add(new GameInput(8.1, (int)DanceAction.StarPower, true));
        inputs.Add(new GameInput(8.2, (int)DanceAction.StarPower, false));
        var ordered = inputs.OrderBy(i => i.Time).ToArray();
        engine.ProcessUpToTime(25, ordered);
        Assert.That(engine.Stats.StarPowerPhrasesHit, Is.EqualTo(2));
        Assert.That(engine.Stats.StarPowerActivationCount, Is.EqualTo(1));
        Assert.That(engine.Stats.StarPowerScore, Is.GreaterThan(0));
        Assert.That(engine.Stats.TimeInStarPower, Is.EqualTo(8).Within(.01));
        Assert.That(engine.GetStarPowerBarAmount(), Is.Zero);
        int score = engine.Stats.TotalScore;
        engine.ProcessUpToTime(6, ordered);
        Assert.That(engine.Stats.StarPowerActivationCount, Is.Zero);
        engine.ProcessUpToTime(25, ordered);
        Assert.That(engine.Stats.TotalScore, Is.EqualTo(score));
        Assert.That(engine.Stats.TotalScore, Is.EqualTo(engine.Stats.NoteScore + engine.Stats.SustainScore + engine.Stats.MultiplierScore + engine.Stats.StarPowerScore));
    }

    [Test]
    public void BothDanceModesDriveTheSharedBandMultiplier()
    {
        var chart = Only(Scan()).LoadChart()!;
        using var dance = new DanceEngine(chart.DanceParts!.GetChart(Instrument.Dance, Difficulty.Expert), chart.SyncTrack, true);
        using var keys = new DanceEngine(chart.DanceParts.GetChart(Instrument.FourKey, Difficulty.Expert), chart.SyncTrack, true);
        var manager = new EngineManager();
        manager.Register(dance, Instrument.Dance, chart, RockMeterPreset.Normal);
        manager.Register(keys, Instrument.FourKey, chart, RockMeterPreset.Normal);
        for (int i = 0; i <= 12 * 120; i++) manager.UpdateEngines(i / 120.0);
        Assert.That(manager.BandMultiplier, Is.EqualTo(4));
        Assert.That(dance.Stats.StarPowerActivationCount, Is.EqualTo(1));
        Assert.That(keys.Stats.StarPowerActivationCount, Is.EqualTo(1));
        Assert.That(dance.Stats.BandBonusScore, Is.GreaterThan(0));
        Assert.That(keys.Stats.BandBonusScore, Is.GreaterThan(0));
        Assert.That(dance.Stats.TotalScore, Is.EqualTo(dance.Stats.NoteScore + dance.Stats.SustainScore + dance.Stats.MultiplierScore + dance.Stats.StarPowerScore));
    }

    [Test]
    public void YaceNoteskinCrossesNativeBoundaryAndUsesChartBeatForMotion()
    {
        var skin = DanceNative.ReadSkin();
        Assert.That(skin.Rotations, Is.EqualTo(new[] { -90f, 180f, 0f, 90f }));
        foreach (var mesh in new[] { skin.Arrow, skin.Receptor, skin.Lift, skin.Mine })
        {
            Assert.That(mesh.Vertices.All(v => v.Length == 8), Is.True);
            Assert.That(mesh.Triangles.All(i => i >= 0 && i < mesh.Vertices.Length), Is.True);
        }
        using var engine = new DanceEngine(Only(Scan()).LoadChart()!.DanceParts!.GetChart(Instrument.Dance, Difficulty.Expert),
            new SyncTrack(480), true);
        Assert.That(engine.VisualBeat(4.1), Is.EqualTo(8).Within(.001), "Arrow motion pauses during the chart's stop");
        Assert.That(DanceNative.yarg_dance_position(0, 10), Is.EqualTo(skin.ReceptorZ));
        Assert.That(skin.ReceptorZ, Is.EqualTo(-2));
        Assert.That(DanceNative.yarg_dance_position(4, 10), Is.GreaterThan(DanceNative.yarg_dance_position(2, 10)));
        Assert.That(DanceNative.yarg_dance_color(.5), Is.EqualTo(0x59bcff));
    }

    [Test]
    public void AllFivePartsAreOneLibrarySongAndKeepIndependentTiming()
    {
        var song = Only(Scan());
        foreach (var part in new[] { Instrument.FiveFretGuitar, Instrument.FourLaneDrums, Instrument.Vocals, Instrument.Dance, Instrument.FourKey })
            Assert.That(song.HasInstrument(part), Is.True, part.ToString());
        var chart = song.LoadChart()!;
        Assert.That(chart.FiveFretGuitar.GetDifficulty(Difficulty.Expert).Notes[0].Time, Is.EqualTo(2));
        Assert.That(chart.FourLaneDrums.GetDifficulty(Difficulty.Expert).Notes[0].Time, Is.EqualTo(2));
        Assert.That(chart.Vocals.IsEmpty, Is.False);
        var dance = chart.DanceParts!.GetChart(Instrument.Dance, Difficulty.Expert);
        Assert.That(dance.Notes.First(n => n.Beat == 9).Time, Is.EqualTo(4.75));
        Assert.That(chart.SyncTrack.TickToTime(9 * 480), Is.EqualTo(4.5));
        Assert.That(chart.DanceParts.GetChart(Instrument.FourKey, Difficulty.Expert).TotalNotes, Is.GreaterThan(dance.TotalNotes));
    }

    [Test]
    public void RefreshIsIdempotentAndQuickScanSeesEditedSimfiles()
    {
        var cache = Scan(); var hash = Only(cache).Hash;
        DanceLibrary.Refresh(cache, new[] { _song });
        Assert.That(Only(cache).Hash, Is.EqualTo(hash));
        var path = Path.Combine(_song, "YACE-pad", "notes.ssc");
        var original = File.ReadAllText(path);
        try
        {
            File.AppendAllText(path, "\n// revision check\n");
            Assert.That(Only(Scan(true)).Hash, Is.Not.EqualTo(hash));
        }
        finally { File.WriteAllText(path, original); }
    }

    [Test]
    public void ChangedSourceRequiresRescanBeforePlay()
    {
        var song = Only(Scan());
        var path = Path.Combine(_song, "YACE-keyboard", "notes.ssc"); var original = File.ReadAllText(path);
        try { File.AppendAllText(path, "\n// stale cache\n"); Assert.Throws<IOException>(() => song.LoadChart()); }
        finally { File.WriteAllText(path, original); }
    }

    [Test]
    public void StandaloneSscLoadsWithoutIniAndPrefersSscOverSm()
    {
        var folder = Path.Combine(_song, "YACE-pad");
        var cache = new SongCache();
        File.WriteAllText(Path.Combine(folder, "old.sm"), "invalid old companion");
        try
        {
            DanceLibrary.Refresh(cache, new[] { folder });
            var song = Only(cache);
            Assert.That(song, Is.TypeOf<DanceSongEntry>());
            Assert.That(song.HasInstrument(Instrument.Dance) && song.HasInstrument(Instrument.FourKey), Is.True);
            Assert.That(song.LoadChart()!.GetEndTime(), Is.GreaterThan(11));
            Assert.That(song.Name.Original, Is.EqualTo("YACE Mixed Band Check"));
        }
        finally { File.Delete(Path.Combine(folder, "old.sm")); }
    }

    [TestCase(0.5)]
    [TestCase(-0.5)]
    public void BandDelayConvertsDanceNotesIntoTheSharedSongClock(double delay)
    {
        var text = File.ReadAllText(Path.Combine(_song, "YACE-pad", "notes.ssc"));
        var original = DanceNative.Parse(text, true).Charts[0];
        var shifted = DanceNative.Parse(text, true, delay).Charts[0];
        Assert.That(shifted.Notes[0].Time, Is.EqualTo(original.Notes[0].Time - delay).Within(1e-9));
        Assert.That(shifted.Notes.Last().Time, Is.EqualTo(original.Notes.Last().Time - delay).Within(1e-9));
    }

    [Test]
    public void FiveBotEnginesPlayTogetherWithoutChangingBandNotes()
    {
        var chart = Only(Scan()).LoadChart()!;
        var thresholds = new[] { .1f, .25f, .45f, .65f, .85f, 1f };
        var preset = EnginePreset.Default;
        var guitar = new YargFiveFretGuitarEngine(chart.FiveFretGuitar.GetDifficulty(Difficulty.Expert).Clone(), chart.SyncTrack,
            preset.FiveFretGuitar.Create(thresholds, new float[6], false), true);
        var drums = new YargDrumsEngine(chart.FourLaneDrums.GetDifficulty(Difficulty.Expert).Clone(), chart.SyncTrack,
            preset.Drums.Create(thresholds, new float[6], DrumsEngineParameters.DrumMode.NonProFourLane), true, false);
        var vocals = new YargVocalsEngine(chart.Vocals.Parts[0].CloneAsInstrumentDifficulty(), chart.SyncTrack,
            preset.Vocals.Create(thresholds, new float[6], Difficulty.Expert, 100, false), true);
        using var dance = new DanceEngine(chart.DanceParts!.GetChart(Instrument.Dance, Difficulty.Expert), chart.SyncTrack, true);
        using var keys = new DanceEngine(chart.DanceParts.GetChart(Instrument.FourKey, Difficulty.Expert), chart.SyncTrack, true);
        var engines = new BaseEngine[] { guitar, drums, vocals, dance, keys };
        var instruments = new[] { Instrument.FiveFretGuitar, Instrument.FourLaneDrums, Instrument.Vocals, Instrument.Dance, Instrument.FourKey };
        var manager = new EngineManager();
        for (int i = 0; i < engines.Length; i++) { engines[i].SetSpeed(1); manager.Register(engines[i], instruments[i], chart, RockMeterPreset.Normal); }
        for (int frame = 0; frame < 16 * 120; frame++) manager.UpdateEngines(frame / 120.0);
        foreach (var engine in engines) Assert.That(engine.BaseStats.NotesHit, Is.GreaterThan(0), engine.GetType().Name);
        Assert.That(dance.Stats.NotesHit, Is.EqualTo(dance.Stats.TotalNotes));
        Assert.That(keys.Stats.NotesHit, Is.EqualTo(keys.Stats.TotalNotes));
        Assert.That(dance.Stats.IsFullCombo && keys.Stats.IsFullCombo, Is.True);
        Assert.That(chart.FiveFretGuitar.GetDifficulty(Difficulty.Expert).Notes[0].Time, Is.EqualTo(2));
    }

    [TestCase(GameMode.Dance, Instrument.Dance)]
    [TestCase(GameMode.FourKey, Instrument.FourKey)]
    public void ProfileAndReplayRoundTripAndReplaySeekingPreserveJudgments(GameMode mode, Instrument instrument)
    {
        var chart = Only(Scan()).LoadChart()!;
        var notes = chart.DanceParts!.GetChart(instrument, Difficulty.Expert);
        using var engine = new DanceEngine(notes, chart.SyncTrack, false);
        var inputs = new List<GameInput>();
        foreach (var note in notes.Notes.Where(n => n.Judgable && n.Kind != "mine"))
        {
            inputs.Add(new GameInput(note.Time, note.Lane, true));
            inputs.Add(new GameInput(Math.Max(note.Time + .001, note.EndTime), note.Lane, false));
        }
        inputs.Add(new GameInput(8.1, (int)DanceAction.StarPower, true));
        inputs.Add(new GameInput(8.2, (int)DanceAction.StarPower, false));
        var ordered = inputs.OrderBy(i => i.Time).ToArray();
        foreach (var original in ordered) { var input = original; engine.QueueInput(ref input); }
        engine.Update(15);
        Assert.That(engine.Stats.StarPowerActivationCount, Is.EqualTo(1));
        var expected = engine.Stats.TotalScore;
        var profile = new YargProfile { Name = "Dance roundtrip", GameMode = mode, CurrentInstrument = instrument, CurrentDifficulty = Difficulty.Expert };
        var frame = new ReplayFrame(profile, engine.Parameters, new DanceStats(engine.Stats), ordered);
        var path = Path.Combine(_temporary, "frame.bin");
        using (var writer = new BinaryWriter(File.Create(path))) frame.Serialize(writer);
        using var bytes = FixedArray.LoadFile(path); var stream = bytes.ToValueStream();
        var replay = new ReplayFrame(ref stream, ReplayIO.REPLAY_VERSIONS.CURRENT);
        Assert.That(replay.Profile.GameMode, Is.EqualTo(mode));
        Assert.That(replay.Profile.CurrentInstrument.ToNativeGameMode(), Is.EqualTo(mode));
        Assert.That(replay.Stats, Is.TypeOf<DanceStats>());
        using var replayEngine = new DanceEngine(notes, chart.SyncTrack, false, (YARG.Core.Engine.Keys.KeysEngineParameters)replay.EngineParameters);
        replayEngine.ProcessUpToTime(15, replay.Inputs);
        Assert.That(replayEngine.Stats.TotalScore, Is.EqualTo(expected));
        replayEngine.ProcessUpToTime(5, replay.Inputs);
        replayEngine.ProcessUpToTime(15, replay.Inputs);
        Assert.That(replayEngine.Stats.TotalScore, Is.EqualTo(expected));
        Assert.That(Marshal.SizeOf<DanceSnapshot>(), Is.EqualTo(56));
    }
}
