using System;
using System.Linq;
using YARG.Core.Chart;
using YARG.Core.Engine;
using YARG.Core.Engine.Keys;
using YARG.Core.Input;
using YARG.Core.IO;

namespace YARG.Core.Dance
{
    public enum DanceAction { Left, Down, Up, Right, StarPower }

    public sealed class DanceStats : KeysStats
    {
        public override bool IsFullCombo => TotalNotes > 0 && NotesHit == TotalNotes && Overhits == 0;
        public DanceStats() { }
        public DanceStats(KeysStats source) : base(source) { }
        public DanceStats(ref FixedArrayStream stream, int version) : base(ref stream, version) { }
    }

    // Native rules/scoring, using the normal YARG engine, band and replay APIs.
    public sealed class DanceEngine : BaseEngine, IDisposable
    {
        public readonly DanceChart Chart;
        public readonly DanceStats Stats = new();
        public readonly KeysEngineParameters Parameters;
        public override BaseEngineParameters BaseParameters => Parameters;
        public override BaseStats BaseStats => Stats;
        public DanceSnapshot Snapshot { get; private set; }
        public DanceScoreState ScoreState { get; private set; }
        public event Action<int>? NotesHit;
        public event Action<int>? NotesMissed;
        public event Action? StarPowerPhraseHit;
        public event Action? StarPowerPhraseMissed;
        private IntPtr _session;
        private double _start = -1e10, _end = 1e10;
        private readonly object _scoreConfig;
        private readonly double[] _eventTimes;
        private int _eventCursor, _pendingAction = -1;
        private bool _pendingPressed;

        public DanceEngine(DanceChart chart, SyncTrack bandSync, bool bot, KeysEngineParameters? parameters = null)
            : base(bandSync, false, bot)
        {
            Chart = chart;
            Parameters = parameters ?? new KeysEngineParameters(
                new HitWindowSettings(0.360, 0.360, 1, false, 0, 1, 1, 0, 0),
                4, 0, 0.250, new[] { .06f, .12f, .20f, .47f, .78f, 1.15f }, new float[6], 0, 0, true, false);
            if (Parameters.SongSpeed <= 0) Parameters.SongSpeed = 1;
            _scoreConfig = DanceScoringConfig.Create(bandSync, chart);
            _eventTimes = chart.Notes.SelectMany(n => new[] { n.Time, n.EndTime }).Distinct().OrderBy(t => t).ToArray();
            Reset();
        }

        public byte State(int index) => DanceNative.yarg_dance_state(_session, (UIntPtr)(uint)index);
        public bool IsStarPowerNote(int index) => StarPowerIsAllowed && DanceNative.yarg_dance_power_note(_session, (UIntPtr)(uint)index);
        public double VisualBeat(double time) => DanceNative.yarg_dance_beat(_session, time);
        public double Distance(int index, double time) => DanceNative.yarg_dance_distance(_session, (UIntPtr)(uint)index, time);
        public int Render(double time, double min, double max, DanceRenderNote[] buffer) =>
            (int)DanceNative.yarg_dance_render(_session, time, min, max, buffer, (UIntPtr)(uint)buffer.Length);

        public void SetRange(double start, double end) { _start = start; _end = end; Reset(); }
        public override void Reset(bool keepCurrentButtons = false)
        {
            base.Reset(keepCurrentButtons);
            InputQueue.Clear();
            DanceNative.yarg_dance_destroy(_session);
            _session = DanceNative.Create(Chart, IsBot, Parameters.SongSpeed, _start, _end);
            DanceNative.ConfigureScore(_session, _scoreConfig);
            _eventCursor = 0; _pendingAction = -1;
            Snapshot = default; ScoreState = default;
            Stats.Reset();
            Stats.TotalNotes = Chart.Notes.Count(n => n.Judgable && n.Kind != "mine" && n.Time >= _start && n.Time < _end);
            ReadScore();
            BaseScore = ScoreState.BaseScore; BaseNoteScore = ScoreState.BaseNoteScore;
            StarScoreThresholds = Parameters.StarMultiplierThresholds.Select(f => (int)Math.Floor(f * BaseScore)).ToArray();
            StarPowerTickPosition = PreviousStarPowerTickPosition = StarPowerTickActivationPosition = StarPowerTickEndPosition = 0;
            StarPowerActivationTime = StarPowerEndTime = BaseTimeInStarPower = 0;
        }
        public override void SetSpeed(double speed)
        {
            base.SetSpeed(speed);
            if (!DanceNative.yarg_dance_set_speed(_session, speed)) throw new ArgumentOutOfRangeException(nameof(speed));
        }
        private void ReadScore()
        {
            if (!DanceNative.yarg_dance_score(_session, out var state)) throw new InvalidOperationException("Could not read Dance scoring");
            ScoreState = state;
            Stats.NoteScore = state.Note;
            Stats.SustainScore = state.Sustain;
            Stats.MultiplierScore = state.Multiplier;
            Stats.StarPowerScore = state.StarPowerScore;
            Stats.CommittedScore = state.Note + state.Sustain + state.Multiplier + state.StarPowerScore;
            Stats.BandBonusScore = state.BandBonus;
            Stats.ScoreMultiplier = state.ScoreMultiplier;
            Stats.TotalChords = state.TotalChords;
            Stats.TotalStarPowerPhrases = state.TotalPhrases;
            Stats.StarPowerPhrasesHit = state.PhrasesHit;
            Stats.StarPowerTickAmount = (uint)Math.Round(state.Power * TicksPerFullSpBar);
            Stats.TotalStarPowerTicks = (uint)Math.Round(state.TotalPower * TicksPerFullSpBar);
            Stats.TotalStarPowerBarsFilled = state.TotalPower;
            Stats.StarPowerActivationCount = state.Activations;
            Stats.TimeInStarPower = state.PowerTime;
            Stats.IsStarPowerActive = state.Active != 0;
            StarPowerEndTime = state.PowerEnd;
        }
        private void Apply(DanceSnapshot snapshot)
        {
            var previous = ScoreState;
            int hit = snapshot.Hits - Snapshot.Hits;
            int missed = snapshot.Misses + snapshot.Drops + snapshot.Mines - Snapshot.Misses - Snapshot.Drops - Snapshot.Mines;
            int combo = Snapshot.Combo;
            ReadScore();
            Stats.NotesHit = snapshot.Hits; Stats.Combo = snapshot.Combo; Stats.MaxCombo = snapshot.MaxCombo;
            Stats.Overhits = snapshot.Mines + snapshot.Drops; Stats.TotalNotes = snapshot.Total;
            NoteIndex = snapshot.NextNote;
            int star = 0;
            while (star < StarScoreThresholds.Length && Stats.TotalScore > StarScoreThresholds[star] && BaseScore > 0) star++;
            float progress = 0;
            if (star < StarScoreThresholds.Length)
            {
                int previousPoints = star > 0 ? StarScoreThresholds[star - 1] : 0;
                int next = StarScoreThresholds[star];
                if (next > previousPoints) progress = Math.Clamp((Stats.TotalScore - previousPoints) / (float)(next - previousPoints), 0, 1);
            }
            Stats.Stars = star + progress;
            if (snapshot.Sequence != Snapshot.Sequence && snapshot.Judgment is > 0 and <= 5) Stats.AddOffsetSample(snapshot.Offset);
            Snapshot = snapshot;
            for (int i = previous.PhrasesHit; i < ScoreState.PhrasesHit; i++) StarPowerPhraseHit?.Invoke();
            for (int i = previous.PhrasesMissed; i < ScoreState.PhrasesMissed; i++) StarPowerPhraseMissed?.Invoke();
            if (previous.Power < .5 && ScoreState.Power >= .5 && !Stats.IsStarPowerActive) OnStarPowerReady?.Invoke();
            if (previous.Active != ScoreState.Active)
            {
                if (Stats.IsStarPowerActive) { StarPowerActivationTime = CurrentTime; StarPowerTickActivationPosition = StarPowerTickPosition; }
                OnStarPowerStatus?.Invoke(Stats.IsStarPowerActive);
            }
            if (missed > 0) { OnComboReset?.Invoke(); NotesMissed?.Invoke(missed); }
            if (hit > 0) { OnComboIncrement?.Invoke(missed > 0 ? snapshot.Combo : Math.Max(snapshot.Combo - combo, 0)); NotesHit?.Invoke(hit); }
        }
        protected override void GenerateQueuedUpdates(double nextTime)
        {
            while (_eventCursor < _eventTimes.Length && _eventTimes[_eventCursor] <= CurrentTime) _eventCursor++;
            while (_eventCursor < _eventTimes.Length && _eventTimes[_eventCursor] < nextTime)
                QueueUpdateTime(_eventTimes[_eventCursor++], "Dance note/hold boundary");
            if (Stats.IsStarPowerActive && ScoreState.PowerEnd > CurrentTime && ScoreState.PowerEnd < nextTime)
                QueueUpdateTime(ScoreState.PowerEnd, "Dance star power ends");
        }
        protected override void MutateStateWithInput(GameInput input)
        {
            if (input.Action is < 0 or > 4) return;
            _pendingAction = input.Action; _pendingPressed = input.Button;
            if (input.Action == 4) IsStarPowerInputActive = input.Button;
        }
        protected override void UpdateHitLogic(double time)
        {
            DanceNative.yarg_dance_score_context(_session, Stats.BandMultiplier, StarPowerIsAllowed);
            if (!DanceNative.yarg_dance_update(_session, time, _pendingAction, _pendingPressed, out var result))
                throw new InvalidOperationException("Reset Dance engine before seeking backwards");
            _pendingAction = -1;
            Apply(result);
        }
        protected override void UpdateTimeVariables(double time)
        {
            LastUpdateTime = CurrentTime; CurrentTime = time;
            LastTick = CurrentTick; CurrentTick = SyncTrack.TimeToTick(time);
            PreviousStarPowerTickPosition = StarPowerTickPosition;
            StarPowerTickPosition = SyncTrack.QuarterTickToMeasureTick(CurrentTick);
        }
        protected override void UpdateBot(double time) { }
        protected override void UpdateStarPower() { }
        protected override void RebaseSustains(uint baseTick) { }
        public override void AllowStarPower(bool isAllowed) => StarPowerIsAllowed = isAllowed;
        public override (double FrontEnd, double BackEnd) CalculateHitWindow() => (-.180 * Parameters.SongSpeed, .180 * Parameters.SongSpeed);
        public void Dispose() { DanceNative.yarg_dance_destroy(_session); _session = IntPtr.Zero; GC.SuppressFinalize(this); }
        ~DanceEngine() => Dispose();
    }
}
