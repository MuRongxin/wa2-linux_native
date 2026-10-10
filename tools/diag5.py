import sys, os, json, time, tempfile; sys.path.insert(0, "src")
import numpy as np, librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, is_matched, _audio_order_bounds)
from index_tts_gui.core import fingerprint as FP

SRC, SR, N = "projects/Fix Bug", 16000, 60
wav_src = collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj = json.load(open(os.path.join(SRC,"project.json"), encoding="utf-8"))
pauses = proj["pauses"][:N]; sents = proj["sentences"][:N]
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)
INTER_S, INTER_X = 3, 1.5
INTRA_S, INTRA_AT, INTRA_X = 40, 0, 0.8
RESYNTH, DELETE = 20, 30

orig, sr0 = librosa.load(wav_src[RESYNTH-1], sr=None, mono=True)
resynth = np.clip(librosa.resample(orig, orig_sr=sr0, target_sr=int(sr0*1.03))*0.9,-1,1).astype(np.float32)
print("句20 原时长 %.3f  重合成后 %.3f" % (durations[RESYNTH-1], len(resynth)/sr0))
takes=[]
for i in range(N):
    y,_ = librosa.load(wav_src[i], sr=SR, mono=True)
    takes.append(resynth if i+1==RESYNTH else y)

slice_off=[pause_offsets(wav_src[i], durations[i]) for i in range(N)]
fps={}
for i,w in enumerate(wav_src,1):
    f=FP.compute_file(w)
    if f: fps[i]=f

def sil(d): return np.zeros(max(1,int(round(d*SR))),dtype=np.float32)
parts,true_start,cur=[],{},0
for i in range(1,N+1):
    if i==DELETE: continue
    y=takes[i-1]; offs=slice_off[i-1]
    if i==INTRA_S and offs:
        c=int(offs[INTRA_AT]*SR); y=np.concatenate([y[:c],sil(INTRA_X),y[c:]])
    true_start[i]=cur/SR; parts.append(y); cur+=len(y)
    p=pauses[i-1]+(INTER_X if i==INTER_S else 0.0)
    parts.append(sil(p)); cur+=len(parts[-1])
adj=np.concatenate(parts)
td=tempfile.mkdtemp(); adjp=os.path.join(td,"e.wav"); sf.write(adjp,adj,SR)
res=align_sentences_detailed(adjp,wav_src,sents,pauses,slice_off,fps)
print()
for i in (18,19,20,21,29,30,31,32,33):
    ts = true_start.get(i)
    print("句%-3d conf=%.3f start=%8.2f 真实=%s  误差=%s" % (
        i, res.scores[i-1], res.starts[i-1],
        ("%.2f"%ts) if ts is not None else "已删除",
        ("%+.3f"%(res.starts[i-1]-ts)) if ts is not None else "-"))
b=_audio_order_bounds(N,res.starts,res.scores,len(adj)/SR)
for i in (20,30):
    print("句%d 搜索区间 %s" % (i, tuple(round(x,2) for x in b.get(i,(0,0)))))
