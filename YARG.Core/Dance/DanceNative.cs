using System;
using System.Runtime.InteropServices;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace YARG.Core.Dance
{
    // Platform interop only. Parsing, timing and all note judgments live in Rust.
    public static class DanceNative
    {
        private const string Library = "yarg_dance";

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        public static extern uint yarg_dance_abi();
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        private static extern IntPtr yarg_dance_skin();
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        public static extern uint yarg_dance_color(double beat);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        public static extern float yarg_dance_position(float distance, float noteSpeed);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern double yarg_dance_beat(IntPtr session, double time);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        internal static extern bool yarg_dance_score(IntPtr session, out DanceScoreState score);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        private static extern bool yarg_dance_configure_score(IntPtr session, byte[] config);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void yarg_dance_score_context(IntPtr session, int band, [MarshalAs(UnmanagedType.I1)] bool allowed);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        internal static extern bool yarg_dance_power_note(IntPtr session, UIntPtr index);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        private static extern IntPtr yarg_dance_parse(byte[] text, [MarshalAs(UnmanagedType.I1)] bool ssc, double clockOffset);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        private static extern void yarg_dance_free_string(IntPtr text);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        private static extern IntPtr yarg_dance_create(byte[] chart, [MarshalAs(UnmanagedType.I1)] bool bot,
            double speed, double start, double end);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void yarg_dance_destroy(IntPtr session);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        internal static extern bool yarg_dance_set_speed(IntPtr session, double speed);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        internal static extern bool yarg_dance_update(IntPtr session, double time, int lane,
            [MarshalAs(UnmanagedType.I1)] bool pressed, out DanceSnapshot snapshot);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern byte yarg_dance_state(IntPtr session, UIntPtr note);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern double yarg_dance_distance(IntPtr session, UIntPtr note, double time);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern UIntPtr yarg_dance_render(IntPtr session, double time, double min, double max,
            [Out] DanceRenderNote[] notes, UIntPtr capacity);

        private static byte[] Utf8(string value) => Encoding.UTF8.GetBytes(value + "\0");

        public static DanceSkinData ReadSkin()
        {
            var pointer = yarg_dance_skin();
            if (pointer == IntPtr.Zero) throw new InvalidOperationException("Could not create YACE noteskin");
            try { return JsonConvert.DeserializeObject<DanceSkinData>(Marshal.PtrToStringUTF8(pointer)!)!; }
            finally { yarg_dance_free_string(pointer); }
        }

        public static DanceSongData Parse(string text, bool ssc, double clockOffset = 0)
        {
            if (yarg_dance_abi() != 1) throw new InvalidOperationException("Incompatible Dance native library");
            var pointer = yarg_dance_parse(Utf8(text), ssc, clockOffset);
            try
            {
                var result = JObject.Parse(Marshal.PtrToStringUTF8(pointer)!);
                if (result["error"] != null) throw new FormatException((string)result["error"]!);
                return result.ToObject<DanceSongData>()!;
            }
            finally { yarg_dance_free_string(pointer); }
        }

        internal static IntPtr Create(DanceChart chart, bool bot, double speed, double start, double end)
        {
            var handle = yarg_dance_create(Utf8(JsonConvert.SerializeObject(chart)), bot, speed, start, end);
            if (handle == IntPtr.Zero) throw new InvalidOperationException("Could not create Dance engine");
            return handle;
        }
        internal static void ConfigureScore(IntPtr handle, object config)
        {
            if (!yarg_dance_configure_score(handle, Utf8(JsonConvert.SerializeObject(config))))
                throw new InvalidOperationException("Invalid Dance scoring configuration");
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct DanceSnapshot
    {
        public int Score, Hits, Combo, MaxCombo, Misses, Drops, Mines, Holds;
        public int Total, Judgment, Sequence, NextNote;
        public double Offset;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct DanceScoreState
    {
        public int Note, Sustain, Multiplier, StarPowerScore, BandBonus, ScoreMultiplier,
            PhrasesHit, TotalPhrases, Activations, Active, BaseScore, BaseNoteScore, TotalChords, PhrasesMissed;
        public double Power, TotalPower, PowerTime, PowerEnd;
    }

    public sealed class DanceSkinData
    {
        public DanceMeshData Arrow = null!, Receptor = null!, Lift = null!, Mine = null!;
        public float[] Rotations = null!;
        public float ReceptorZ, ScrollFactor;
    }
    public sealed class DanceMeshData
    {
        public float[][] Vertices = null!;
        public int[] Triangles = null!;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct DanceRenderNote
    {
        public float Head, Tail;
        public int Lane, Kind, State, Index;
    }
}
