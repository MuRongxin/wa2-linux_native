import json, os
import numpy as np, librosa
from index_tts_gui.core.merger import collect_sentence_wavs
SRC="projects/Fix Bug"
wavs=collect_sentence_wavs(os.path.join(SRC,"output_tts"))
d=json.load(open("/tmp/calib_adj_241s7t2e/result.json"))
ts={int(k):v for k,v in d["true_start"].items()}
out=d["out"]
got={}
for p,(idx,st,en) in enumerate(out):
    orig=p+1 if p<199 else p+2
    got[orig]=st
bad=[78,121,506,518]
print("这 4 句的音频到底有多安静:")
for i in bad:
    y,sr=librosa.load(wavs[i-1],sr=None,mono=True)
    rms=float(np.sqrt(np.mean(y.astype(np.float64)**2)))
    peak=float(np.max(np.abs(y))) if len(y) else 0
    print("  句%-4d 样本%6d  RMS=%.6f  峰值=%.6f  -> 校准到 %.2f (真实 %.2f)"%(
        i,len(y),rms,peak,got.get(i,-1),ts.get(i,-1)))
print()
print("静音/极短句在整个项目里的数量:")
short=0; quiet=0
for i,w in enumerate(wavs,1):
    if get:=None: pass
    y,sr=librosa.load(w,sr=8000,mono=True)
    if len(y)/sr < 0.15: short+=1
    elif float(np.sqrt(np.mean(y.astype(np.float64)**2))) < 0.005: quiet+=1
print("  <0.15s: %d 句   近乎静音(<0.005 RMS): %d 句"%(short,quiet))
