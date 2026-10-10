import sys, os, tempfile; sys.path.insert(0, "src")
import numpy as np
import librosa
from index_tts_gui.core import fingerprint as FP

SR = FP.FINGERPRINT_SR

def render(formants, syl_dur, f0_base, seed):
    rng = np.random.default_rng(seed)
    out = []
    for dur in syl_dur:
        n = int(SR*dur); t = np.arange(n)/SR
        x = np.zeros(n)
        for (fr, bw, amp) in formants:
            x += amp*np.exp(-np.pi*bw*t)*np.cos(2*np.pi*fr*t + rng.uniform(0, 6.28))
        f0 = f0_base*(1 + 0.05*np.sin(2*np.pi*1.5*t + rng.uniform(0, 6.28)))
        x *= (0.6 + 0.4*np.sin(2*np.pi*np.cumsum(f0)/SR))
        x *= (0.5 + 0.5*np.sin(np.pi*t/dur))
        out.append(x)
    return (np.concatenate(out) + rng.normal(0, 0.003, int(SR*sum(syl_dur)))).astype(np.float32)

VOW_A = [(700, 90, 1.0), (1220, 110, 0.5), (2600, 170, 0.25)]
VOW_B = [(400, 80, 1.0), (1900, 130, 0.4), (2550, 200, 0.2)]
a1 = render(VOW_A, [0.20, 0.22, 0.20], 150, 1)
a2 = render(VOW_A, [0.21, 0.215, 0.205], 137, 2)
b1 = render(VOW_B, [0.21, 0.21, 0.21], 158, 3)
gap = np.zeros(int(SR*0.4), dtype=np.float32)
full = np.concatenate([b1, gap, a2])
t0 = len(b1)/SR + 0.4

fa = FP.compute(a1, SR)
fb = FP.compute(b1, SR)
print("指纹长度 =", len(fa), " 量化往返误差 =",
      float(np.abs(np.array(fa)/FP.QUANT_SCALE - np.array(FP.compute(a1,SR))/FP.QUANT_SCALE).max()))
print("同文本两渲染指纹距离 = %.4f" % FP.distance(fa, FP.compute(a2, SR)))
print("不同文本指纹距离= %.4f" % FP.distance(fa, fb))
print("阈值 = %.2f" % FP.MATCH_THRESHOLD)
print()

HOP = 512
M = librosa.feature.mfcc(y=full, sr=SR, n_mfcc=FP.N_MFCC, hop_length=HOP)
W = int((len(a2)/SR) * SR / HOP)
pos, d = FP.locate(fa, M, HOP, W, 0.0, len(full)/SR, max(1, SR//40//HOP), SR)
print("在全文滑窗定位: pos=%.2fs dist=%.4f  (真实 %.2fs, 误差 %.2fs)" % (pos, d, t0, abs(pos-t0)))

pb, db = FP.locate(fb, M, HOP, W, 0.0, len(full)/SR, max(1, SR//40//HOP), SR)
print("异文本指纹定位: pos=%.2fs dist=%.4f  (真实 %.2fs, 误差 %.2fs)" % (pb, db, 0.0, abs(pb)))
print("  -> 两者都被正确定位" if abs(pos-t0) < 0.15 and abs(pb) < 0.15 else "  -> 有偏差")

print("落盘往返:", end=" ")
with tempfile.TemporaryDirectory() as d2:
    p = FP.save(d2, {1: fa, 2: fb})
    back = FP.load(d2)
    print(FP.distance(back[1], fa) < 1e-9 and FP.distance(back[1], fb) > 0.1)