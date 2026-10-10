import sys, os, json, tempfile; sys.path.insert(0, "src")
import numpy as np, librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.subtitle import SubtitleEntry
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, recalibrate_entries_detailed)
from index_tts_gui.core import fingerprint as FP

SRC, SR, N = "projects/Fix Bug", 16000, 60
wav_src = collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj = json.load(open(os.path.join(SRC,"project.json"), encoding="utf-8"))
pauses = proj["pauses"][:N]; sents = proj["sentences"][:N]
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)

orig, sr0 = librosa.load(wav_src[19], sr=None, mono=True)
resynth = librosa.resample(orig, orig_sr=sr0, target_sr=int(sr0*1.03))
print("句20: sr=%d 原时长 %.3fs -> resynth %.3fs" % (sr0, len(orig)/sr0, len(resynth)/(sr0*1.03)))
slice_off = [pause_offsets(wav_src[i], durations[i]) for i in range(N)]
fps = {i: FP.compute_file(w) for i, w in enumerate(wav_src,1)}
fps = {k:v for k,v in fps.items() if v}

def sil(d): return np.zeros(max(1,int(round(d*SR))), dtype=np.float32)
takes = []
for i in range(N):
    y,_ = librosa.load(wav_src[i], sr=SR, mono=True)
    takes.append(np.clip(resynth*0.9,-1,1).astype(np.float32) if i+1==20 else y)
parts, true_start, cur = [], {}, 0
for i in range(1, N+1):
    if i == 30: continue
    y = takes[i-1]; offs = slice_off[i-1]
    if i == 40 and offs:
        c = int(offs[0]*SR); y = np.concatenate([y[:c], sil(0.8), y[c:]])
    true_start[i] = cur/SR; parts.append(y); cur += len(y)
    p = pauses[i-1] + (1.5 if i == 3 else 0.0)
    parts.append(sil(p)); cur += len(parts[-1])
adj = np.concatenate(parts)
d = tempfile.mkdtemp(); adjp = os.path.join(d,"a.wav"); sf.write(adjp, adj, SR)
print("音频 %.2fs (ffprobe %.2fs)" % (len(adj)/SR, get_wav_duration(adjp)))
print("true_start[54] = %.2f" % true_start[54])

entries = [SubtitleEntry(i, old_starts[i-1], old_starts[i-1]+durations[i-1], sents[i-1][:12]) for i in range(1,N+1)]
res = align_sentences_detailed(adjp, wav_src, sents, pauses, slice_off, fps)
print("starts[53] (句54) = %.2f   starts[19] (句20) = %.2f" % (res.starts[53], res.starts[19]))
out, rep = recalibrate_entries_detailed(entries, old_starts, durations, res.starts, slice_off, res.slice_deltas)
print("out 条数 =", len(out), " 丢弃 =", rep.dropped)
print("out[53].start = %.2f" % out[53].start_sec)