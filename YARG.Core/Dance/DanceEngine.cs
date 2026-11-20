using System;
using System.Linq;
using YARG.Core.Chart;
using YARG.Core.Engine;
using YARG.Core.Engine.Keys;
using YARG.Core.Input;
using YARG.Core.IO;

namespace YARG.Core.Dance
{
    public enum DanceAction { Left, Down, Up, Right }

    // Reuses the versioned keys replay envelope, but retains dance FC semantics:
    // mines and broken holds invalidate an otherwise complete run.
    public sealed class DanceStats : KeysStats
    {
        public override bool IsFullCombo => TotalNotes > 0 && NotesHit == TotalNotes && Overhits == 0;
        public DanceStats() { }
        public DanceStats(KeysStats source) : base(source) { }
        public DanceStats(ref FixedArrayStream stream, int version) : base(ref stream, version) { }
    }

    public sealed class DanceEngine : BaseEngine, IDisposable
    {
        public readonly DanceChart Chart;
        public readonly DanceStats Stats = new();
        public readonly KeysEngineParameters Parameters;
        public override BaseEngineParameters BaseParameters => Parameters;
        public override BaseStats BaseStats => Stats;
        public DanceSnapshot Snapshot { get; private set; }
        public event Action<int>? NotesHit;
        public event Action<int>? NotesMissed;
        private IntPtr _session;
        private double _start = -1e10, _end = 1e10;
        private int _rawScore;

        public DanceEngine(DanceChart chart, SyncTrack bandSync, bool bot, KeysEngineParameters? parameters = null)
            : base(bandSync, true, bot)
        {
            Chart = chart;
            Parameters = parameters ?? new KeysEngineParameters(
                new HitWindowSettings(0.360, 0.360, 1, false, 0, 1, 1, 0, 0),
                1, 0, 0.250, new[] { .10f, .25f, .45f, .65f, .85f, 1f }, new float[6], 0, 0, true, false);
            if (Parameters.SongSpeed <= 0) Parameters.SongSpeed = 1;
            Reset();
        }

        public byte State(int index) => DanceNative.yarg_dance_state(_session, (UIntPtr)(uint)index);
        public double VisualBeat(double time) => DanceNative.yarg_dance_beat(_session, time);
        public double Distance(int index, double time) => DanceNative.yarg_dance_distance(_session, (UIntPtr)(uint)index, time);
        public int Render(double time, double min, double max, DanceRenderNote[] buffer) =>
            (int)DanceNative.yarg_dance_render(_session, time, min, max, buffer, (UIntPtr)(uint)buffer.Length);

        public void SetRange(double start, double end)
        {
            _start = start; _end = end; Reset();
        }

        public override void Reset(bool keepCurrentButtons = false)
        {
            base.Reset(keepCurrentButtons);
            InputQueue.Clear();
            DanceNative.yarg_dance_destroy(_session);
            _session = DanceNative.Create(Chart, IsBot, Parameters.SongSpeed, _start, _end);
            Snapshot = default;
            _rawScore = 0;
            Stats.Reset();
            Stats.TotalNotes = Chart.Notes.Count(n => n.Judgable && n.Kind != "mine" && n.Time >= _start && n.Time < _end);
            Stats.TotalChords = Stats.TotalNotes;
            BaseScore = BaseNoteScore = Stats.TotalNotes * 1000 + Chart.Notes.Count(n => n.Judgable && n.IsSustain && n.Time >= _start && n.Time < _end) * 250;
            StarScoreThresholds = Parameters.StarMultiplierThresholds.Select(f => (int)(f * BaseScore)).ToArray();
        }

        public override void SetSpeed(double speed)
        {
            base.SetSpeed(speed);
            if (!DanceNative.yarg_dance_set_speed(_session, speed)) throw new ArgumentOutOfRangeException(nameof(speed));
        }

        private void Apply(DanceSnapshot snapshot)
        {
            int hit = snapshot.Hits - Snapshot.Hits;
            int missed = snapshot.Misses + snapshot.Drops + snapshot.Mines - Snapshot.Misses - Snapshot.Drops - Snapshot.Mines;
            if (snapshot.Score > _rawScore)
                Stats.BandBonusScore += (snapshot.Score - _rawScore) * Math.Max(0, Stats.BandBonusMultiplier);
            _rawScore = snapshot.Score;
            Stats.CommittedScore = snapshot.Score + Stats.BandBonusScore;
            Stats.NoteScore = snapshot.Score;
            Stats.NotesHit = snapshot.Hits;
            Stats.Combo = snapshot.Combo;
            Stats.MaxCombo = snapshot.MaxCombo;
            Stats.Overhits = snapshot.Mines + snapshot.Drops;
            Stats.TotalNotes = snapshot.Total;
            Stats.ScoreMultiplier = 1;
            NoteIndex = snapshot.NextNote;
            var thresholds = StarScoreThresholds;
            int star = 0;
            while (star < thresholds.Length && Stats.TotalScore >= thresholds[star] && BaseScore > 0) star++;
            Stats.Stars = star;
            if (snapshot.Sequence != Snapshot.Sequence && snapshot.Judgment <= 5 && snapshot.Judgment > 0)
            {
                Stats.AddOffsetSample(snapshot.Offset);
            }
            Snapshot = snapshot;
            if (hit > 0) { OnComboIncrement?.Invoke(hit); NotesHit?.Invoke(hit); }
            if (missed > 0) { OnComboReset?.Invoke(); NotesMissed?.Invoke(missed); }
        }

        protected override void MutateStateWithInput(GameInput input)
        {
            if (input.Action is < 0 or > 3) return;
            if (!DanceNative.yarg_dance_update(_session, input.Time, input.Action, input.Button, out var result))
                throw new InvalidOperationException("Dance input time moved backwards");
            Apply(result);
        }
        protected override void UpdateHitLogic(double time)
        {
            if (!DanceNative.yarg_dance_update(_session, time, -1, false, out var result))
                throw new InvalidOperationException("Reset Dance engine before seeking backwards");
            Apply(result);
        }
        protected override void UpdateTimeVariables(double time)
        {
            LastUpdateTime = CurrentTime; CurrentTime = time;
            LastTick = CurrentTick; CurrentTick = SyncTrack.TimeToTick(time);
        }
        protected override void UpdateBot(double time) { }
        protected override void UpdateStarPower() { }
        protected override void RebaseSustains(uint baseTick) { }
        public override void AllowStarPower(bool isAllowed) { }
        public override (double FrontEnd, double BackEnd) CalculateHitWindow() => (-0.180 * Parameters.SongSpeed, 0.180 * Parameters.SongSpeed);
        public void Dispose()
        {
            DanceNative.yarg_dance_destroy(_session); _session = IntPtr.Zero;
            GC.SuppressFinalize(this);
        }
        ~DanceEngine() => Dispose();
    }
}
