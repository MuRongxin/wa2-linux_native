import sys; sys.path.insert(0, "src")
import numpy as np
import librosa
from index_tts_gui.core.speech_aligner import _cross_correlate_match

SR = 16000

def render(formants, syl_dur, f0_base, seed):
    """同一段文本的两次渲染：共振峰（音色/音素）完全相同，只有
    F0 轮廓、相位、时长微扰、噪声不同 —— 这就是两遍 TTS 的差别。"""
    rng = np.random.default_rng(seed)
    out = []
    for dur in syl_dur:
        n = int(SR * dur)
        t = np.arange(n) / SR
        x = np.zeros(n)
        for (fr, bw, amp) in formants:
            env = np.exp(-np.pi * bw * t)
            x += amp * env * np.cos(2*np.pi*fr*t + rng.uniform(0, 6.28))
        f0 = f0_base * (1 + 0.05*np.sin(2*np.pi*1.5*t + rng.uniform(0, 6.28)))
        x *= (0.6 + 0.4*np.sin(2*np.pi*np.cumsum(f0)/SR))
        x *= (0.5 + 0.5*np.sin(np.pi * t / dur))
        out.append(x)
    y = np.concatenate(out) + rng.normal(0, 0.003, int(SR*sum(syl_dur)))
    return y.astype(np.float32)

def fp(y, n_mfcc=20):
    S = librosa.feature.mfcc(y=y, sr=SR, n_mfcc=n_mfcc)
    return np.concatenate([S[1:].mean(axis=1), S[1:].std(axis=1)])

def fp_cos(f, g):
    na, nb = np.linalg.norm(f), np.linalg.norm(g)
    if na < 1e-9 or nb < 1e-9:
        return 1.0
    return float(1.0 - (f @ g)/(na*nb))

VOW_A = [(700, 90, 1.0), (1220, 110, 0.5), (2600, 170, 0.25)]
VOW_B = [(400, 80, 1.0), (1900, 130, 0.4), (2550, 200, 0.2)]

a1 = render(VOW_A, [0.20, 0.22, 0.20], 150, 1)
a2 = render(VOW_A, [0.21, 0.215, 0.205], 137, 2)
b1 = render(VOW_B, [0.21, 0.21, 0.21], 158, 3)
gap = np.zeros(int(SR*0.4), dtype=np.float32)
full = np.concatenate([b1, gap, a2])
t0 = len(b1)/SR + 0.4
print("布局: B[0,%.2f) 间隔 A2[%.2f,%.2f)  A2时长=%.2f" % (len(b1)/SR, t0, len(full)/SR, len(a2)/SR))

st, cf = _cross_correlate_match(a1, full, SR, 0.0)
print("NCC(A渲染1, 修改后) = %.3f  -> 波形匹配%s" % (cf, "可用" if cf >= 0.5 else "失效（正是需要指纹的场景）"))
print()
fa, fb = fp(a1), fp(b1)
print("在目标真实位置 t0=%.2f 处:" % t0)
seg = full[int(t0*SR):int(t0*SR)+len(a1)]
da_true, db_true = fp_cos(fa, fp(seg)), fp_cos(fb, fp(seg))
print("   dist(同文本指纹)=%.4f   dist(异文本指纹)=%.4f   margin=%+.4f" % (da_true, db_true, db_true-da_true))
print("在异文本位置 t=0 处:")
seg_b = full[:len(a1)]
print("   dist(同文本指纹)=%.4f   dist(异文本指纹)=%.4f   margin=%+.4f" % (fp_cos(fa, fp(seg_b)), fp_cos(fb, fp(seg_b)), fp_cos(fb, fp(seg_b))-fp_cos(fa, fp(seg_b))))
print()
print("滑窗找最小值:")
pad = np.zeros(SR, np.float32)
sig = np.concatenate([full, pad])
W = int(SR * (len(a2)/SR))
rows = []
for pos in range(0, len(full), SR//40):
    win = sig[pos:pos+W]
    if len(win) < W: break
    rows.append((pos/SR, fp_cos(fa, fp(win)), fp_cos(fb, fp(win))))
rows.sort(key=lambda r: r[1])
for p, da, db in rows[:3]:
    print("   t=%.2f dist(同)=%.4f dist(异)=%.4f margin=%+.4f 误差=%.2fs" % (p, da, db, db-da, abs(p-t0)))