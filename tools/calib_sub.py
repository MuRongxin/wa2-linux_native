import sys, os, json, time, tempfile; sys.path.insert(0, "src")
import numpy as np
import librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.subtitle import SubtitleEntry
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, recalibrate_entries_detailed,
    report_lines, is_matched)
from index_tts_gui.core import fingerprint as FP

SRC, SR, N = "projects/Fix Bug", 16000, 60
all_wavs = collect_sentence_wavs(os.path.join(SRC, "output_tts"))
wav_src = all_wavs[:N]
proj = json.load(open(os.path.join(SRC, "project.json"), encoding="utf-8"))
sentences, pauses = proj["sentences"][:N], proj["pauses"][:N]
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)

INTER_S, INTER_X = 3, 1.5
INTRA_S, INTRA_AT, INTRA_X = 40, 0, 0.8
RESYNTH, DELETE = 20, 30

orig, sr0 = librosa.load(wav_src[RESYNTH-1], sr=None, mono=True)
resynth = np.clip(librosa.resample(orig, orig_sr=sr0, target_sr=int(sr0*1.03))*0.9, -1, 1).astype(np.float32)
takes = []
for i in range(N):
    y, _ = librosa.load(wav_src[i], sr=SR, mono=True)
    takes.append(resynth if i+1 == RESYNTH else y)

print("准备切片几何与指纹…", flush=True)
t0=time.time()
slice_off = [pause_offsets(wav_src[i], durations[i]) for i in range(N)]
fps = {}
for i, w in enumerate(wav_src, 1):
    f = FP.compute_file(w)
    if f: fps[i] = f
print("  用时 %.1fs; 有内部停顿的句子 %d/%d; 指纹 %d 条" % (
      time.time()-t0, sum(1 for s in slice_off if s), N, len(fps)), flush=True)

def sil(d): return np.zeros(max(1,int(round(d*SR))), dtype=np.float32)
parts, true_start, cur = [], {}, 0
for i in range(1, N+1):
    if i == DELETE: continue
    y = takes[i-1]; offs = slice_off[i-1]
    if i == INTRA_S and offs:
        c = int(offs[INTRA_AT]*SR)
        y = np.concatenate([y[:c], sil(INTRA_X), y[c:]])
    true_start[i] = cur/SR; parts.append(y); cur += len(y)
    p = pauses[i-1] + (INTER_X if i == INTER_S else 0.0)
    parts.append(sil(p)); cur += len(parts[-1])
adj = np.concatenate(parts)
tmpd = tempfile.mkdtemp(); adjp = os.path.join(tmpd, "edited.wav")
sf.write(adjp, adj, SR)
entries = [SubtitleEntry(i, old_starts[i-1], old_starts[i-1]+durations[i-1], sentences[i-1][:16]) for i in range(1, N+1)]
print("调整后 %.1fs  (句%d后+%s / 句%d内部+%s / 句%d重合成 / 句%d删除)" % (
      len(adj)/SR, INTER_S, INTER_X, INTRA_S, INTRA_X, RESYNTH, DELETE), flush=True)
print("对齐中…", flush=True)
t1=time.time()
res = align_sentences_detailed(adjp, wav_src, sentences, pauses, slice_off, fps)
t_align=time.time()-t1
out, rep = recalibrate_entries_detailed(entries, old_starts, durations, res.starts, slice_off, res.slice_deltas)
print("对齐 %.1fs / 重映射 %.1fs" % (t_align, time.time()-t1-t_align), flush=True)
print("整句匹配 %d/%d" % (sum(1 for i in range(N) if is_matched(res.starts[i], res.scores[i])), N), flush=True)
print("句%d(重合成) conf=%.3f 起点 %.2f (真实 %.2f, 误差 %+.3f)" % (
      RESYNTH, res.scores[RESYNTH-1], res.starts[RESYNTH-1], true_start[RESYNTH], res.starts[RESYNTH-1]-true_start[RESYNTH]), flush=True)
print("报告:", rep.summary(), flush=True)
for l in report_lines(rep, limit=5): print(l, flush=True)
# 注意：recalibrate_entries_detailed 会对保留下来的字幕重新连续编号，
# 所以不能用 index 反查原句号（丢弃几条之后后面全部错位）。用文本对齐。
_by_text = {}
for _e in out: _by_text.setdefault(_e.text, _e)
by = {}
for _i in range(1, N+1):
    if _i == DELETE: continue
    _e = _by_text.get(sentences[_i-1][:16])
    if _e is not None: by[_i] = _e
errs = sorted(((abs(by[i].start_sec-true_start[i]), i) for i in true_start if i in by), reverse=True)
print("起点误差 中位 %.3fs / 90分位 %.3fs / 最大 %.3fs" % (
      np.median([e[0] for e in errs]), np.percentile([e[0] for e in errs],90), errs[0][0]), flush=True)
print("误差最大 5 句:", flush=True)
for err,i in errs[:5]:
    print("   句%-3d 误差 %.3fs  得到 %.2f  真实 %.2f" % (i, err, by[i].start_sec, true_start[i]), flush=True)
print("丢弃:", rep.dropped, " 句%d已删除被正确丢弃: %s" % (DELETE, DELETE in rep.dropped), flush=True)
e400 = by.get(INTRA_S)
if e400:
    print("句%d(内部插入) 字幕 [%.2f, %.2f]，原时长 %.2f -> 应拉伸约 %.2fs" % (
          INTRA_S, e400.start_sec, e400.end_sec, durations[INTRA_S-1], INTRA_X), flush=True)