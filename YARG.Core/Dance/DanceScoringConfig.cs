using System;
using System.Linq;
using Newtonsoft.Json;
using YARG.Core.Chart;

namespace YARG.Core.Dance
{
    public sealed class DancePowerPhrase
    {
        [JsonProperty("start")] public double Start;
        [JsonProperty("end")] public double End;
    }

    // SDK clock transport. Scoring, sustain integration and SP rules are Rust.
    internal static class DanceScoringConfig
    {
        public static object Create(SyncTrack sync, DanceChart chart)
        {
            var ticks = sync.Tempos.Select(t => t.Tick).Concat(sync.TimeSignatures.Select(s => s.Tick)).Append(0u).Distinct().OrderBy(t => t);
            var clock = ticks.Select(tick =>
            {
                var tempo = sync.Tempos.LastOrDefault(t => t.Tick <= tick);
                var sig = sync.TimeSignatures.LastOrDefault(s => s.Tick <= tick);
                double bps = (tempo?.BeatsPerMinute ?? 120) / 60;
                double beatsPerMeasure = sig == null ? 4 : sig.Numerator * 4.0 / sig.Denominator;
                return new { time = sync.TickToTime(tick), beat = tick / (double)sync.Resolution,
                    measure = sync.QuarterTickToMeasureTick(tick) / (double)sync.MeasureResolution,
                    beats_per_second = bps, measures_per_second = bps / beatsPerMeasure };
            }).ToArray();
            return new { clock, phrases = chart.PowerPhrases };
        }
    }
}
