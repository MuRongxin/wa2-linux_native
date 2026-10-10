import sys, os, json, time, tempfile; sys.path.insert(0, "src")
import numpy as np
import librosa

from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, recalibrate_entries_detailed,
    report_lines, CONFIDENCE_THRESHOLD, is_matched)
from index_tts_gui.core import fingerprint as FP

SRC = "projects/Fix Bug"
SR = 16000
wav_src = collect_sentence_wavs(os.path.join(SRC, "output_tts"))
proj = json.load(open(os.path.join(SRC, "project.json"), encoding="utf-8"))
sentences, pauses = proj["sentences"], proj["pauses"]
n = len(wav_src)
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)

INTER_GAP_SENT, INTER_EXTRA = 3, 1.5      # 句3 之后的间隔 +1.5s
INTRA_SENT, INTRA_AT, INTRA_EXTRA = 400, 1, 0.8   # 句400 内部第1个停顿 +0.8s
RESYNTH = 200# 模拟"重新合成"
DELETE = 300# 删掉

# 磁盘上的 take：句200 换成重新合成的版本（其余保持合并时那一版）
orig200, sr200 = librosa.load(wav_src[RESYNTH-1], sr=None, mono=True)
resynth200 = librosa.resample(orig200, orig_sr=sr200, target_sr=int(sr200*1.03))
resynth200 = np.clip(resynth200*0.9, -1, 1).astype(np.float32)
takes = []
for i in range(1, n+1):
    y, _ = librosa.load(wav_src[i-1], sr=SR, mono=True)
    takes.append(resynth200 if i == RESYNTH else y)

# 指纹在"合并时"落盘：描述 full_dub.wav 里那一版，即原始 take
t0 = time.time()
slice_off = []
fps = {}
for i, w in enumerate(wav_src, 1):
    slice_off.append(pause_offsets(w, durations[i-1]))
    if i % 1 == 0:
        f = FP.compute_file(w)
        if f:
            fps[i] = f
print("准备: %d 句切片几何 + %d 条指纹, %.1fs" % (len(slice_off), len(fps), time.time()-t0))
print("  有内部停顿的句子: %d / %d" % (sum(1 for s in slice_off if s), n))

# ── 构造"调整后"音频（用户在剪辑软件里做的事）──
def sil(d):
    return np.zeros(max(1, int(round(d*SR))), dtype=np.float32)

parts, true_start, cur = [], {}, 0
for i in range(1, n+1):
    if i == DELETE:
        continue
    y = takes[i-1]
    offs = slice_off[i-1]
    if i == INTRA_SENT and offs:
        # 在内部停顿处插入静音 —— 注意用的是"原始 take"的停顿位置
        cut = int(offs[INTRA_AT]*SR)
        y = np.concatenate([y[:cut], sil(INTRA_EXTRA), y[cut:]])
    true_start[i] = cur / SR
    parts.append(y); cur += len(y)
    p = pauses[i-1] + (INTER_EXTRA if i == INTER_GAP_SENT else 0.0)
    parts.append(sil(p)); cur += len(parts[-1])
adjusted = np.concatenate(parts)
import soundfile as sf
tmp = tempfile.mkdtemp(prefix="calib_adj_")
adj_path = os.path.join(tmp, "full_dub_edited.wav")
sf.write(adj_path, adjusted, SR)
print("调整后音频: %.1fs  (%s)" % (len(adjusted)/SR, adj_path))
print("  句3后 间隔 +%.1fs / 句400内部 +%.1fs / 句%d重新合成 / 句%d已删除"%(
      INTER_EXTRA, INTRA_EXTRA, RESYNTH, DELETE))

# 原始字幕（每句一条，与默认流水线一致）
from index_tts_gui.core.subtitle import SubtitleEntry
entries = [SubtitleEntry(i, old_starts[i-1], old_starts[i-1]+durations[i-1],
                         sentences[i-1][:20]) for i in range(1, n+1)]

t0 = time.time()
res = align_sentences_detailed(adj_path, wav_src, sentences, pauses,
                                slice_off, fps)
t_align = time.time()-t0
matched = sum(1 for i in range(n) if is_matched(res.starts[i], res.scores[i]))
print()
print("对齐耗时 %.1fs (%d 句, %.1f 分钟音频)" % (t_align, n, old_starts[-1]/60))
print("  整句匹配 %d / %d" % (matched, n))
print("  句%d(重新合成) 置信度 %.3f  起点 %.2f (真实 %.2f, 误差 %+.2f)"%(
      RESYNTH, res.scores[RESYNTH-1], res.starts[RESYNTH-1], true_start[RESYNTH],
      res.starts[RESYNTH-1]-true_start[RESYNTH]))

out, report = recalibrate_entries_detailed(
    entries, old_starts, durations, res.starts, slice_off, res.slice_deltas)
t_remap = time.time()-t0-t_align
print("重映射耗时 %.1fs" % t_remap)
print("报告:", report.summary())
for line in report_lines(report, limit=4):
    print(line)

# ── 与真实值逐句对比 ──
# recalibrate_entries_detailed 会对保留下来的字幕重新连续编号，
# 丢弃几条之后后面全部错位，所以不能直接用 index 反查原句号。
# out 的第 p 条(0-based)对应原句 p+1+已丢弃条数(在 p 之前)。
by_idx = {}
_p = 0
_drop_at = {d-1 for d in report.dropped}
for _e in out:
    while _p in _drop_at: _p += 1
    by_idx[_p+1] = _e
    _p += 1
errs = []
for i in range(1, n+1):
    if i not in true_start:
        continue
    e = by_idx.get(i)
    if e is None:
        continue
    errs.append((abs(e.start_sec - true_start[i]), i, e.start_sec, true_start[i]))
errs.sort(reverse=True)
print()
print("起点误差: 中位 %.3fs  90分位 %.3fs  最大 %.3fs" % (
      np.median([x[0] for x in errs]),
      np.percentile([x[0] for x in errs], 90),
      errs[0][0]))
print("误差最大的 5 句:")
for err, i, got, exp in errs[:5]:
    print("   句%-4d 误差 %.3fs  得到 %.2f  真实 %.2f" % (i, err, got, exp))
print()
print("被丢弃的句:", report.dropped[:10], "(共%d)" % len(report.dropped))
print("句%d(已删除) 是否被正确丢弃: %s" % (DELETE, DELETE in report.dropped))
json.dump({"true_start": true_start, "out": [(i, by_idx[i].start_sec, by_idx[i].end_sec) for i in sorted(by_idx)]},
          open(os.path.join(tmp, "result.json"), "w"))
print("临时目录:", tmp)