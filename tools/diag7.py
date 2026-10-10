import sys, os, json; sys.path.insert(0,"src")
import numpy as np, librosa, soundfile as sf, tempfile
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.speech_aligner import cumulative_starts
from index_tts_gui.core import fingerprint as FP

SRC,SR,N="projects/Fix Bug",16000,80
wavs=collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj=json.load(open(os.path.join(SRC,"project.json"),encoding="utf-8"))
pauses=proj["pauses"][:N]
durs=[get_wav_duration(w) for w in wavs]
starts=cumulative_starts(durs,pauses)
parts=[]
for w,p in zip(wavs,pauses):
    parts.append(librosa.load(w,sr=SR,mono=True)[0])
    parts.append(np.zeros(max(1,int(round(p*SR))),dtype=np.float32))
y=np.concatenate(parts)
td=tempfile.mkdtemp(); ref=os.path.join(td,"ref.wav"); sf.write(ref,y,SR)
print("参考音频 %.1fs / %d 句" % (len(y)/SR, N))

M=librosa.feature.mfcc(y=y,sr=FP.FINGERPRINT_SR,n_mfcc=FP.N_MFCC,hop_length=FP.HOP_LENGTH)
hop=FP.HOP_LENGTH; body=M[1:]
step=max(1,int(0.05*SR//hop))
ok=0; tot=0; rows=[]
for i in range(0, N, 4):
    fpv=FP.compute_file(wavs[i])
    if not fpv: continue
    va=FP._dequantize(fpv); na=float(np.linalg.norm(va))
    frames=max(1,int(durs[i]*SR/hop)); nfr=M.shape[1]
    lo=max(0,int((starts[i]-2.0)*SR/hop)); hi=min(nfr-frames,int((starts[i]+2.0)*SR/hop))
    best=(9.9,-1.0)
    for s in range(lo,hi+1,step):
        win=body[:,s:s+frames]
        if win.shape[1]<frames: break
        vb=np.concatenate([win.mean(axis=1),win.std(axis=1)])
        nb=float(np.linalg.norm(vb))
        if nb<1e-9: continue
        d=float(1.0-(va@vb)/(na*nb))
        if d<best[0]: best=(d,s*hop/SR)
    err=abs(best[1]-starts[i]) if best[1]>=0 else 999
    tot+=1; ok+=(err<=0.5)
    rows.append((i+1,best[0],best[1],starts[i],err))
print("同一段音频内，指纹最佳命中误差<=0.5s: %d/%d" % (ok,tot))
for r in rows[:14]: print("  句%-3d dist=%.4f 命中%7.2f 真实%7.2f 误差%6.2f" % r)
