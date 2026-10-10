import sys, os, json, tempfile; sys.path.insert(0, "src")
import numpy as np, librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, SentenceTimeMap)
from index_tts_gui.core import fingerprint as FP

SRC, SR, N = "projects/Fix Bug", 16000, 60
wav_src = collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj = json.load(open(os.path.join(SRC,"project.json"), encoding="utf-8"))
pauses = proj["pauses"][:N]; sents = proj["sentences"][:N]
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)
slice_off = [pause_offsets(wav_src[i], durations[i]) for i in range(N)]
fps = {i: FP.compute_file(w) for i,w in enumerate(wav_src,1)}
fps = {k:v for k,v in fps.items() if v}

def sil(d): return np.zeros(max(1,int(round(d*SR))), dtype=np.float32)
takes=[]
orig,sr0 = librosa.load(wav_src[19], sr=None, mono=True)
rs = np.clip(librosa.resample(orig,orig_sr=sr0,target_sr=int(sr0*1.03))*0.9,-1,1).astype(np.float32)
for i in range(N):
    y,_=librosa.load(wav_src[i],sr=SR,mono=True)
    takes.append(rs if i+1==20 else y)
parts,true_start,cur=[],{},0
for i in range(1,N+1):
    if i==30: continue
    y=takes[i-1]; offs=slice_off[i-1]
    if i==40 and offs:
        c=int(offs[0]*SR); y=np.concatenate([y[:c],sil(0.8),y[c:]])
    true_start[i]=cur/SR; parts.append(y); cur+=len(y)
    p=pauses[i-1]+(1.5 if i==3 else 0.0)
    parts.append(sil(p)); cur+=len(parts[-1])
d=tempfile.mkdtemp(); adjp=os.path.join(d,"a.wav"); sf.write(adjp,np.concatenate(parts),SR)
res = align_sentences_detailed(adjp, wav_src, sents, pauses, slice_off, fps)

i = 53   # 句54
e_start = old_starts[i]
sd = res.starts[i] - old_starts[i]
tm = SentenceTimeMap(durations[i], slice_off[i], res.slice_deltas[i], sd)
print("句54: entry.start=%.2f  starts=%.2f  sentence_delta=%.3f" % (e_start, res.starts[i], sd))
print("  slice_off[53] =", [round(x,2) for x in slice_off[i]])
print("  slice_deltas[53] =", [None if x is None else round(x,3) for x in res.slice_deltas[i]])
print("  tm._bounds =", [round(x,2) for x in tm._bounds])
k = tm.slice_of(e_start)
print("  slice_of(entry.start) =", k, "-> delta_of =", tm.delta_of(k))
print("  map(entry.start) =", tm.map(e_start))
print("  has_slice_info =", tm.has_slice_info(), " len(_deltas) =", len(tm._deltas))