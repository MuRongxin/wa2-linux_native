import sys, math, wave, struct; sys.path.insert(0, "src")
import numpy as np
from index_tts_gui.core.merger import get_wav_duration
from index_tts_gui.core.speech_aligner import align_sentences, cumulative_starts, recalibrate_entries
from index_tts_gui.core.subtitle import SubtitleEntry

SR = 8000
def write_wav(path, samples):
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1,min(1,s))*32000)) for s in samples))
def tone(freq, dur):
    t = np.arange(int(SR*dur))/SR
    return (0.6*np.sin(2*np.pi*freq*t)).astype(np.float32)
def sil(dur):
    return np.zeros(int(SR*dur), dtype=np.float32)

import tempfile, os
d = tempfile.mkdtemp()

# ── 两个分句 ──
#句1 = "前半，后半。"  TTS 会在逗号处自然停顿 -> 建模为 音+音
head = tone(440, 0.9); comma_gap = sil(0.25); tail = tone(660, 0.8)
s1 = np.concatenate([head, comma_gap, tail])       # 2.05s
s2 = tone(880, 1.2)                # 1.2s
p1 = os.path.join(d, "sentence_01_x.wav"); write_wav(p1, s1)
p2 = os.path.join(d, "sentence_02_y.wav"); write_wav(p2, s2)
dur = [get_wav_duration(p1), get_wav_duration(p2)]
pauses = [0.3, 0.0]
print(f"原始:句1={dur[0]:.2f}s 句2={dur[1]:.2f}s")

# ── 默认流水线的字幕：一分句一条 ──
entries = [SubtitleEntry(1, 0.0, dur[0], "前半，后半。"),
           SubtitleEntry(2, dur[0]+pauses[0], dur[0]+pauses[0]+dur[1], "下一句")]
old_starts = cumulative_starts(dur, pauses)
print("原字幕:", [(round(e.start_sec,2), round(e.end_sec,2), e.text) for e in entries])

# ── 用户在句1 的逗号停顿处额外插入 0.5s 间隔；句2 不动 ──
INSERT = 0.5
s1_mod = np.concatenate([head, comma_gap, sil(INSERT), tail])
full = np.concatenate([s1_mod, sil(pauses[0]), s2])
fullp = os.path.join(d, "full.wav"); write_wav(fullp, full)
print(f"\n剪辑后: 句1内部 +{INSERT}s -> 总长 {len(s1_mod)/SR:.2f}s，句2 位置后移 {INSERT}s")

new_starts, scores = align_sentences(fullp, [p1, p2], ["前半，后半。", "下一句"], pauses)
for i,(ns,sc) in enumerate(zip(new_starts, scores)):
    print(f"  句{i+1}: new_start={ns:.3f} conf={sc:.3f}")

out, dropped = recalibrate_entries(entries, old_starts, dur, new_starts)
print("\n校准结果:", [(round(e.start_sec,2), round(e.end_sec,2)) for e in out], "dropped=", dropped)
#真实期望：句1 起点不变、终点应+0.5；句2 整体+0.5
print("期望:句1 [%.2f, %.2f]句2 [%.2f, %.2f]" % (
    0.0, dur[0]+INSERT, dur[0]+pauses[0]+INSERT, dur[0]+pauses[0]+INSERT+dur[1]))
err1 = out[0].end_sec - (dur[0]+INSERT)
err2 = out[1].end_sec - (dur[0]+pauses[0]+INSERT+dur[1])
print(f"\n句1 尾部误差 {err1:+.2f}s ；句2 尾部误差 {err2:+.2f}s")