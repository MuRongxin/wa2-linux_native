import sys; sys.path.insert(0, "src")
from index_tts_gui.core.speech_aligner import recalibrate_entries, sentence_of, cumulative_starts
from index_tts_gui.core.subtitle import SubtitleEntry

# ── 原始布局 ──
#  A 语音 [0.0, 2.0)   间隔 [2.0, 2.3)   B 语音 [2.3, 4.0)
old_durations = [2.0, 1.7]
old_pauses    = [0.3, 0.0]
old_starts    = cumulative_starts(old_durations, old_pauses)
print("old_starts =", [round(s,3) for s in old_starts])

# 一条横跨 A 尾 + B 头的字幕：文本 = A的后半 + B的前半
cue = SubtitleEntry(1, 1.8, 2.5, "聊聊/这个话题")
mid = (cue.start_sec + cue.end_sec) / 2
owner = sentence_of(mid, old_starts)
print(f"cue=[{cue.start_sec}, {cue.end_sec})  mid={mid:.2f}  -> 归属句 index={owner}")
print("   （注意 end=2.5 已越过 B 的起点 2.3，但归属只看中点）")

# ── 剪辑后：A/B 交换 + 间隔改变 ──
#  B 语音 [0.0, 1.7)   间隔 [1.7, 2.5)   A 语音 [2.5, 4.5)
print()
print("情形一：仅改间隔（A 不动, B 后移 0.2）")
new_starts = [0.0, 2.5]
out, dropped = recalibrate_entries([cue], old_starts, old_durations, new_starts)
print("   delta: A=%+.2f  B=%+.2f" % (new_starts[0]-old_starts[0], new_starts[1]-old_starts[1]))
print("   结果:", [(round(e.start_sec,3), round(e.end_sec,3)) for e in out], "dropped=", dropped)
print("   期望: 起点用 A 的 delta(+0.00)，终点用 B 的 delta(+0.20) -> [1.80, 2.70)")

print("情形二：交换次序 + 改间隔（B 到句首, A 到句尾）")
new_starts = [2.5, 0.0]
out, dropped = recalibrate_entries([cue], old_starts, old_durations, new_starts)
print("   delta: A=%+.2f  B=%+.2f" % (new_starts[0]-old_starts[0], new_starts[1]-old_starts[1]))
print("   结果:", [(round(e.start_sec,3), round(e.end_sec,3)) for e in out], "dropped=", dropped)
print("   A 实际在 [2.5, 4.5)，B 实际在 [0.0, 1.7)")
print("   -> 字幕落在 B 之后很远的地方，与两侧语音都对不上")