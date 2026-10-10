import sys, os, wave, struct, tempfile; sys.path.insert(0, "src")
import numpy as np
from index_tts_gui.core.merger import get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, recalibrate_entries_detailed)
from index_tts_gui.core.subtitle import SubtitleEntry

SR = 8000
def write_wav(path, x):
    x = np.clip(x, -1, 1)
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(v*32000)) for v in x))
def voiced(f0, dur, seed):
    r = np.random.default_rng(seed)
    t = np.arange(int(SR*dur))/SR
    x = sum(a*np.sin(2*np.pi*f0*k*t + r.uniform(0, 6.28)) for k, a in
            ((1,1.0),(2,.5),(3,.33),(4,.2),(6,.12)))
    env = 0.55 + 0.45*np.sin(2*np.pi*4.5*t - 1.2)
    return (0.7*x*env + r.normal(0, .01, len(x))).astype(np.float32)
def sil(d):
    return np.zeros(int(SR*d), dtype=np.float32)

d = tempfile.mkdtemp()
h1 = voiced(135, 0.9, 1)
t1 = voiced(210, 0.8, 2)
s2 = voiced(180, 1.2, 3)
p1 = os.path.join(d, "sentence_01_a.wav")
write_wav(p1, np.concatenate([h1, sil(0.25), t1]))
p2 = os.path.join(d, "sentence_02_b.wav")
write_wav(p2, s2)
dur = [get_wav_duration(p1), get_wav_duration(p2)]
pauses = [0.3, 0.0]
old_starts = cumulative_starts(dur, pauses)
entries = [SubtitleEntry(1, old_starts[0], old_starts[0]+dur[0], "A"),
           SubtitleEntry(2, old_starts[1], old_starts[1]+dur[1], "B")]

slice_off = [pause_offsets(p1, dur[0]), pause_offsets(p2, dur[1])]
print("句1 内部停顿偏移:", slice_off[0], " 句2:", slice_off[1])
print("原时长: 句1=%.2f 句2=%.2f" % (dur[0], dur[1]))
print("原字幕:", [(round(e.start_sec,2), round(e.end_sec,2)) for e in entries])

INS = 0.5

def run(tag, seg1, seg2):
    full = np.concatenate([seg1, sil(pauses[0]), seg2])
    fp = os.path.join(d, "full_" + tag + ".wav")
    write_wav(fp, full)
    res = align_sentences_detailed(fp, [p1, p2], ["x", "y"], pauses, slice_off)
    out, rep = recalibrate_entries_detailed(
        entries, old_starts, dur, res.starts, slice_off, res.slice_deltas)
    print("")
    print("=== " + tag + " ===")
    print("  starts:", [round(v,2) for v in res.starts])
    print("  slice_deltas:", [[None if x is None else round(x,2) for x in dl]
                              for dl in res.slice_deltas])
    print("  字幕:", [(round(e.start_sec,2), round(e.end_sec,2)) for e in out])
    print("  报告:", rep.summary())
    return out, rep

o, r = run("A", np.concatenate([h1, sil(0.25), sil(INS), t1]), s2)
print("  期望: [(%.2f, %.2f), (%.2f, %.2f)]" % (old_starts[0], dur[0]+INS, dur[0]+INS+pauses[0], dur[0]+INS+pauses[0]+dur[1]))
o, r = run("B", np.concatenate([sil(0.9), sil(0.25), t1]), s2)
o, r = run("C", np.concatenate([t1, sil(0.25+INS), h1]), s2)