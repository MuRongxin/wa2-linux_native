import sys, os, json; sys.path.insert(0,"src")
import numpy as np, librosa
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.speech_aligner import cumulative_starts
from index_tts_gui.core import fingerprint as FP

SRC,SR,N="projects/Fix Bug",16000,40
wavs=collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj=json.load(open(os.path.join(SRC,"project.json"),encoding="utf-8"))
pauses=proj["pauses"][:N]
durs=[get_wav_duration(w) for w in wavs]
starts=cumulative_starts(durs,pauses)
hop=FP.HOP_LENGTH
step=max(1,int(0.05*SR//hop))
rng=np.random.default_rng(7)

def mfeat(y):
    return librosa.feature.mfcc(y=y,sr=SR,n_mfcc=FP.N_MFCC,hop_length=hop)

def best_in(M,fpv,lo,hi,frames):
    body=M[1:]; va=FP._dequantize(fpv); na=float(np.linalg.norm(va))
    nfr=M.shape[1]
    f_lo=max(0,int(lo*SR/hop)); f_hi=min(nfr-frames,int(hi*SR/hop))
    if f_hi<f_lo: return 9.9,-1.0
    best=(9.9,-1.0)
    for s in range(f_lo,f_hi+1,step):
        win=body[:,s:s+frames]
        if win.shape[1]<frames: break
        vb=np.concatenate([win.mean(axis=1),win.std(axis=1)])
        nb=float(np.linalg.norm(vb))
        if nb<1e-9: continue
        d=float(1.0-(va@vb)/(na*nb))
        if d<best[0]: best=(d,s*hop/SR)
    return best

CASES={
 "原样(仅重编码)": lambda y: y,
 "加噪 sigma=0.02": lambda y: y+rng.normal(0,0.02,len(y)).astype(np.float32),
 "音量 x0.6": lambda y: (y*0.6),
 "重采样 x1.03(变调变速)": lambda y: librosa.resample(y,orig_sr=SR,target_sr=int(SR*1.03)).astype(np.float32),
 "高通+低通(音色变化)": lambda y: librosa.effects.preemphasis(y.astype(np.float32)),
}
print("%-26s %6s %6s %8s %8s"%("扰动","误判","总数","真位置距","最佳距"))
for name,fn in CASES.items():
    parts=[]
    for w,p in zip(wavs,pauses):
        yy=librosa.load(w,sr=SR,mono=True)[0]
        z=fn(yy)
        if len(z)<SR//4: z=np.pad(z,(0,SR//4-len(z)))
        parts.append(z.astype(np.float32))
        parts.append(np.zeros(max(1,int(round(p*SR))),dtype=np.float32))
    M=mfeat(np.concatenate(parts))
    bad=0; tot=0; errs=[]; ds=[]
    for i in range(2,N,4):
        fpv=FP.compute_file(wavs[i])
        if not fpv: continue
        frames=max(1,int(durs[i]*SR/hop))
        d,pos=best_in(M,fpv,max(0,starts[i]-2.0),starts[i]+2.0,frames)
        if pos<0: continue
        err=abs(pos-starts[i]); tot+=1; errs.append(err); ds.append(d)
        if err>0.5: bad+=1
    print("%-26s %6d %6d %8.3f %8.4f"%(name,bad,tot,np.median(errs) if errs else -1,np.median(ds) if ds else -1))
