import sys, os, json, tempfile; sys.path.insert(0, "src")
import numpy as np, librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitle import SubtitleEntry
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, recalibrate_entries_detailed)

SRC, SR, N = "projects/Fix Bug", 16000, 12
wav_src = collect_sentence_wavs(os.path.join(SRC, "output_tts"))[:N]
proj = json.load(open(os.path.join(SRC,"project.json"), encoding="utf-8"))
pauses = proj["pauses"][:N]; sents = proj["sentences"][:N]
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)

def sil(d): return np.zeros(max(1,int(round(d*SR))), dtype=np.float32)
takes = [librosa.load(w, sr=SR, mono=True)[0] for w in wav_src]
parts, true_start, cur = [], {}, 0
for i in range(1, N+1):
    true_start[i] = cur/SR
    parts.append(takes[i-1]); cur += len(takes[i-1])
    parts.append(sil(pauses[i-1])); cur += len(parts[-1])
adj = np.concatenate(parts)
d = tempfile.mkdtemp(); adjp = os.path.join(d,"a.wav"); sf.write(adjp, adj, SR)
print("音频 %.2fs, 无任何编辑（应当全部对上）" % (len(adj)/SR))

res = align_sentences_detailed(adjp, wav_src, sents, pauses)   # 不传指纹/切片
print("starts :", [round(v,2) for v in res.starts])
print("true   :", [round(true_start[i],2) for i in range(1,N+1)])
print("scores :", [round(v,2) for v in res.scores])
print("最大起点误差 %.3fs" % max(abs(res.starts[i]-true_start[i+1]) for i in range(N)))

entries = [SubtitleEntry(i, old_starts[i-1], old_starts[i-1]+durations[i-1], sents[i-1][:12]) for i in range(1,N+1)]
out, rep = recalibrate_entries_detailed(entries, old_starts, durations, res.starts)
print("映射后 starts:", [round(e.start_sec,2) for e in out])
print("报告:", rep.summary())