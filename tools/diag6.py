import sys, os, json, tempfile; sys.path.insert(0, "src")
import numpy as np, librosa, soundfile as sf
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.speech_aligner import cumulative_starts
from index_tts_gui.core import fingerprint as FP

SRC, SR, N = "projects/Fix Bug", 16000, 60
wav_src = collect_sentence_wavs(os.path.join(SRC,"output_tts"))[:N]
proj = json.load(open(os.path.join(SRC,"project.json"), encoding="utf-8"))
pauses = proj["pauses"][:N]
durations=[get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)
print("句20 旧起点 %.2f 句30 旧起点 %.2f" % (old_starts[19], old_starts[29]))
orig,sr0 = librosa.load(wav_src[19], sr=None, mono=True)
resynth = np.clip(librosa.resample(orig,orig_sr=sr0,target_sr=int(sr0*1.03))*0.9,-1,1).astype(np.float32)
takes=[]
for i in range(N):
    y,_=librosa.load(wav_src[i], sr=SR, mono=True)
    takes.append(resynth if i==19 else y)
def sil(d): return np.zeros(max(1,int(round(d*SR))),dtype=np.float32)
INTER_S,INTER_X,INTRA_S,INTRA_AT,INTRA_X,DELETE=3,1.5,40,0,0.8,30
from index_tts_gui.core.subtitler import pause_offsets
slice_off=[pause_offsets(wav_src[i],durations[i]) for i in range(N)]
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
hop=FP.HOP_LENGTH
M=librosa.feature.mfcc(y=adj,sr=FP.FINGERPRINT_SR,n_mfcc=FP.N_MFCC,hop_length=hop)
step=max(1,int(FP.SEARCH_STEP_SEC*FP.FINGERPRINT_SR//hop))
body=M[1:]
def scan(fp,lo,hi):
    va=FP._dequantize(fp); na=float(np.linalg.norm(va))
    nfr=M.shape[1]
    w=max(1,int(durations[0]*0))
    return None
def full_scan(fpi,lo,hi,win_sec):
    fpf=FP.compute_file(wav_src[fpi-1])
    va=FP._dequantize(fpf); na=float(np.linalg.norm(va))
    frames=max(1,int(win_sec*FP.FINGERPRINT_SR/hop))
    nfr=M.shape[1]
    f_lo=max(0,int(lo*FP.FINGERPRINT_SR/hop)); f_hi=min(nfr-frames,int(hi*FP.FINGERPRINT_SR/hop))
    ds=[]
    for s in range(f_lo,f_hi+1,max(1,step)):
        win=body[:,s:s+frames]
        if win.shape[1]<frames: break
        vb=np.concatenate([win.mean(axis=1),win.std(axis=1)])
        nb=float(np.linalg.norm(vb))
        if nb<1e-9: continue
        ds.append((float(1.0-(va@vb)/(na*nb)), s*hop/FP.FINGERPRINT_SR))
    ds.sort()
    return ds
for idx,(label,lo,hi) in [(20,("重合成",76.27,83.36)),(30,("已删除",108.52,110.19))]:
    ds=full_scan(idx,lo,hi,durations[idx-1])
    if not ds: print(label,"无窗口"); continue
    print("\n句%d(%s) 窗口内最佳 5 个距离:" % (idx,label))
    for d,p in ds[:5]:
        print("   dist=%.4f @%.2fs%s" % (d,p,"  <=真实位置" if abs(p-true_start.get(idx,-9))<1.5 else ""))
    arr=np.array([d for d,_ in ds])
    print("   距离分布: min=%.4f p05=%.4f 中位=%.4f" % (arr.min(), np.percentile(arr,5), np.median(arr)))
    print("   阈值0.08 之下有 %d/%d 个候选" % ((arr<=0.08).sum(), len(arr)))
