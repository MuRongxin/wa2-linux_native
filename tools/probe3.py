import sys, os, wave, struct, tempfile; sys.path.insert(0, "src")
import numpy as np
from index_tts_gui.core.merger import get_wav_duration
from index_tts_gui.core.speech_aligner import align_sentences, cumulative_starts, recalibrate_entries
from index_tts_gui.core.subtitle import SubtitleEntry

SR = 8000
rng = np.random.default_rng(7)
def write_wav(path, x):
    x = np.clip(x, -1, 1)
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(v*32000)) for v in x))
def voiced(f0, dur, seed):
    # 类语音: 谐波堆叠 + 音节包络 + 轻噪, 避免纯音的退化互相关
    r = np.random.default_rng(seed)
    t = np.arange(int(SR*dur))/SR
    x = sum(a*np.sin(2*np.pi*f0*k*t + r.uniform(0, 6.28)) for k, a in
            ((1,1.0),(2,.5),(3,.33),(4,.2),(6,.12)))
    env = 0.55 + 0.45*np.sin(2*np.pi*4.5*t - 1.2)
    return (0.7*x*env + r.normal(0, .01, len(x))).astype(np.float32)
def sil(dur):
    return np.zeros(int(SR*dur), dtype=np.float32)

d = tempfile.mkdtemp()
h1 = voiced(135, 0.9, 1)      # 句1 前半
t1 = voiced(210, 0.8, 2)      # 句1 后半（逗号后）
s2 = voiced(180, 1.2, 3)      # 句2
p1 = os.path.join(d,"sentence_01_a.wav"); write_wav(p1, np.concatenate([h1, sil(0.25), t1]))
p2 = os.path.join(d,"sentence_02_b.wav"); write_wav(p2, s2)
dur = [get_wav_duration(p1), get_wav_duration(p2)]; pauses=[0.3, 0.0]
old_starts = cumulative_starts(dur, pauses)
entries = [SubtitleEntry(1, old_starts[0], old_starts[0]+dur[0], "前半，后半。"),
           SubtitleEntry(2, old_starts[1], old_starts[1]+dur[1], "下一句")]
print(f"原始 句1={dur[0]:.2f}s 句2={dur[1]:.2f}s")
print("原字幕:", [(round(e.start_sec,2), round(e.end_sec,2)) for e in entries])

def run(tag, seg1, seg2):
    full = np.concatenate([seg1, sil(pauses[0]), seg2])
    fp = os.path.join(d, f"full_{tag}.wav"); write_wav(fp, full)
    ns, sc = align_sentences(fp, [p1,p2], ["前半，后半。","下一句"], pauses)
    out, dropped = recalibrate_entries(entries, old_starts, dur, ns)
    print(f"\n=== {tag} ===")
    print("  align:", [f"{v:.2f}(c={c:.2f})" for v,c in zip(ns,sc)], "dropped=", dropped)
    for e in out:
        print(f"    字幕 [{e.start_sec:.2f}, {e.end_sec:.2f}]")
    return out

INS = 0.5
o = run("A 句1逗号处+0.5s", np.concatenate([h1, sil(0.25), sil(INS), t1]), s2)
exp = [(old_starts[0], old_starts[0]+dur[0]+INS), (old_starts[0]+dur[0]+INS+pauses[0], old_starts[0]+dur[0]+INS+pauses[0]+dur[1])]
print("  期望:", [(round(a,2), round(b,2)) for a,b in exp])
for k,(e,(a,b)) in enumerate(zip(o, exp)):
    print(f"    句{k+1}: 起点误差 {e.start_sec-a:+.2f}s, 终点误差 {e.end_sec-b:+.2f}s")

o = run("B 删掉句1前半(只剩后半)", np.concatenate([sil(0.9), sil(0.25), t1]), s2)
print("  （句1 音频已不连续 —— 期望是明确报错或提示，而非静默错位）")

o = run("C 句1两半对调", np.concatenate([t1, sil(0.25+INS), h1]), s2)
print("  （句1 内部顺序被调换 —— 单条连续字幕无法表达）")