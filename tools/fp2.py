import sys; sys.path.insert(0, "src")
import numpy as np
import librosa
from index_tts_gui.core.speech_aligner import _cross_correlate_match

SR = 16000

def render(formants, syl_dur, f0_base, seed, tilt=0.0):
    rng = np.random.default_rng(seed)
    out = []
    for dur in syl_dur:
        n = int(SR * dur); t = np.arange(n) / SR
        f0 = f0_base * (1.0 + 0.06 * np.sin(2 * np.pi * 1.7 * t))
        phase = 2 * np.pi * np.cumsum(f0) / SR
        x = np.zeros(n)
        for (fr, bw, amp) in formants:
            x += amp * np.exp(-np.pi*bw*t) * np.cos(phase*fr/np.maximum(f0,1e-9)*f0 + rng.uniform(0, 6))
        x *= (0.5 + 0.5 * np.sin(np.pi * t / max(dur, 1e-6)))
        out.append(x)
    y = np.concatenate(out) + rng.normal(0, 0.004, int(SR * sum(syl_dur)))
    if tilt:
        y = y * np.exp(-tilt * np.arange(len(y)) / SR)
    return y.astype(np.float32)

def fp(y, n_mfcc=20):
    S = librosa.feature.mfcc(y=y, sr=SR, n_mfcc=n_mfcc)
    return np.concatenate([S[1:].mean(axis=1), S[1:].std(axis=1)])

def fp_cos(f, g):
    na, nb = np.linalg.norm(f), np.linalg.norm(g)
    if na < 1e-9 or nb < 1e-9:
        return 1.0
    return float(1.0 - (f @ g) / (na * nb))

VOW_A = [(700, 90, 1.0), (1220, 110, 0.5), (2600, 170, 0.25)]
VOW_B = [(400, 80, 1.0), (1900, 130, 0.4), (2550, 200, 0.2)]
a1 = render(VOW_A, [0.18, 0.22, 0.20], 150, 1)
a2 = render(VOW_A, [0.19, 0.21, 0.22], 138, 2, tilt=0.6)
b1 = render(VOW_B, [0.20, 0.20, 0.20], 160, 3)
gap = np.zeros(int(SR*0.4), dtype=np.float32)
full = np.concatenate([b1, gap, a2])
t0 = len(b1)/SR + 0.4

st, cf = _cross_correlate_match(a1, full, SR, 0.0)
print("音频布局: B[0,%.2f)  间隔  A2[%.2f,%.2f)" % (len(b1)/SR, t0, len(full)/SR))
print("整句 NCC(A渲染1, 修改后音频) = %.3f @ %.2fs" % (cf, st))
print("  -> 波形匹配%s" % ("可用（此用例无需指纹）" if cf >= 0.5 else "失效，恰好是需要指纹的场景"))
print()
fa, fb = fp(a1), fp(b1)
W = len(a1)   # 窗口就等于原句长，滑窗时内容才对齐
pad = np.zeros(SR, dtype=np.float32)
sig = np.concatenate([full, pad])
rows = []
for pos in range(0, len(full) - W + SR//20, SR//20):
    win = sig[pos:pos+W]
    if len(win) < W:
        break
    rows.append((pos/SR, fp_cos(fa, fp(win)), fp_cos(fb, fp(win))))
rows.sort(key=lambda r: r[1])
print("目标真实位置 t0 = %.2f" % t0)
print("指纹最近前 3:" )
for p, da, db in rows[:3]:
    print("   t=%.2fs dist(同文本)=%.4f dist(异文本)=%.4f margin=%.4f  误差=%.2fs" % (p, da, db, db-da, abs(p-t0)))
print("异文本最佳: t=%.2f" % sorted(rows, key=lambda r: r[2])[0][0])