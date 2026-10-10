import sys, os, json, tempfile; sys.path.insert(0, "src")
import numpy as np, librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.speech_aligner import cumulative_starts

SRC, SR, N = "projects/Fix Bug", 16000, 12
wav_src = collect_sentence_wavs(os.path.join(SRC, "output_tts"))[:N]
proj = json.load(open(os.path.join(SRC, "project.json"), encoding="utf-8"))
pauses = proj["pauses"][:N]
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)
print("durations:", [round(d,2) for d in durations])
print("pauses[:12]:", [round(p,2) for p in pauses[:12]])
print("old_starts:", [round(s,2) for s in old_starts])

def sil(d): return np.zeros(max(1,int(round(d*SR))), dtype=np.float32)
takes = [librosa.load(w, sr=SR, mono=True)[0] for w in wav_src]
print("len(takes)/SR:", [round(len(t)/SR,2) for t in takes])
parts, true_start, cur = [], {}, 0
for i in range(1, N+1):
    y = takes[i-1]
    true_start[i] = cur/SR
    parts.append(y); cur += len(y)
    parts.append(sil(pauses[i-1])); cur += len(parts[-1])
adj = np.concatenate(parts)
d = tempfile.mkdtemp(); adjp = os.path.join(d, "a.wav")
sf.write(adjp, adj, SR)
print("构造音频 %.2fs, ffprobe 读回 %.2fs" % (len(adj)/SR, get_wav_duration(adjp)))
print("true_start:", {k: round(v,2) for k,v in true_start.items()})
print("两套时间轴一致?", all(abs(true_start[i]-old_starts[i-1])<0.05 for i in range(1,N+1)))