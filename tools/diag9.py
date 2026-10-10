import sys, os, json, time; sys.path.insert(0,"src")
import numpy as np, librosa
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.speech_aligner import cumulative_starts

SRC,SR,N="projects/Fix Bug",16000,40
wavs=collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj=json.load(open(os.path.join(SRC,"project.json"),encoding="utf-8"))
pauses=proj["pauses"][:N]
durs=[get_wav_duration(w) for w in wavs]
starts=cumulative_starts(durs,pauses)
HOP=512; rng=np.random.default_rng(7)

def frames_of(y):
    M=librosa.feature.mfcc(y=y.astype(np.float32),sr=SR,n_mfcc=20,hop_length=HOP)
    M=M[1:]                      # 去 c0（增益不敏感）
    n=np.linalg.norm(M,axis=0,keepdims=True)
    return M/np.maximum(n,1e-9)  # 逐帧归一 -> 幅度不敏感

def seq_match(Q,Wf,max_shift_frames):
    """Q:(F,Tq) Wf:(F,T)  -> 最佳对角偏移(帧)与平均余弦相似度"""
    S=Q.T@Wf                      # (Tq, T)
    best=(-1.0,0)
    Tq,T=S.shape
    # 期望位置附近滑动；对角线取平均相似度
    mid=T//2
    lo=max(0,mid-max_shift_frames); hi=min(T-Tq,mid+max_shift_frames)
    for off in range(lo,hi+1):
        v=float(np.mean(S[np.arange(Tq),np.arange(Tq)+off]))
        if v>best[0]: best=(v,off)
    return best

CASES={
 "原样": lambda y: y,
 "加噪0.02": lambda y: y+rng.normal(0,0.02,len(y)).astype(np.float32),
 "音量x0.6": lambda y: y*0.6,
 "重采样x1.03": lambda y: librosa.resample(y,orig_sr=SR,target_sr=int(SR*1.03)).astype(np.float32),
}
for name,fn in CASES.items():
    parts=[]
    for w,p in zip(wavs,pauses):
        yy=librosa.load(w,sr=SR,mono=True)[0]
        z=fn(yy)
        if len(z)<SR//4: z=np.pad(z,(0,SR//4-len(z)))
        parts.append(z.astype(np.float32)); parts.append(np.zeros(max(1,int(round(p*SR))),dtype=np.float32))
    Wf=frames_of(np.concatenate(parts))
    bad=0; tot=0; errs=[]; sims=[]
    t0=time.time()
    for i in range(2,N,4):
        Q=frames_of(librosa.load(wavs[i],sr=SR,mono=True)[0])
        shift=int(round(durs[i]*SR/HOP))
        mid=int(starts[i]*SR//HOP)
        base=max(0, mid-shift)
        seg=Wf[:, base:mid+shift]
        best=seq_match(Q,seg,shift)
        pos=(base+best[1])*HOP/SR
        err=abs(pos-starts[i]); tot+=1; errs.append(err); sims.append(best[0])
        if err>0.5: bad+=1
    print("%-12s 误判 %d/%d  误差中位 %.3fs  相似度中位 %.3f  用时 %.1fs"%(
        name,bad,tot,np.median(errs),np.median(sims),time.time()-t0))
