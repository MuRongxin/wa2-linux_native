import sys, os, wave, struct, tempfile; sys.path.insert(0, "src")
import numpy as np
import librosa

SR = 16000

def render(formants, syl_dur, f0_base, seed, tilt=0.0):
    # 用共振峰合成"类语音"：同一段元音序列 + 不同 F0/相位/噪声/时长 = 两次 TTS 渲染
    rng = np.random.default_rng(seed)
    out = []
    for dur in syl_dur:
        n = int(SR * dur)
        t = np.arange(n) / SR
        f0 = f0_base * (1.0 + 0.06 * np.sin(2 * np.pi * 1.7 * t))
        phase = 2 * np.pi * np.cumsum(f0) / SR
        x = np.zeros(n)
        for (fr, bw, amp) in formants:
            r = np.exp(-np.pi * bw * t) * np.cos(2 * np.pi * fr * t + rng.uniform(0, 6))
            x += amp * r
        x *= (0.5 + 0.5 * np.sin(np.pi * t / max(dur, 1e-6)))
        out.append(x)
    y = np.concatenate(out) + rng.normal(0, 0.004, int(SR * sum(syl_dur)))
    if tilt:
        y = y * np.exp(-tilt * np.arange(len(y)) / SR)
    return y.astype(np.float32)

def norm(a):
    b = a - a.mean()
    nrm = np.linalg.norm(b)
    return b / nrm if nrm > 1e-9 else b

def ncc_best(template, sig):
    import scipy.signal
    t = norm(template.astype(np.float64))
    s = sig.astype(np.float64)
    num = scipy.signal.correlate(s, t, mode="valid")
    return float(np.max(num) / max(np.linalg.norm(s) * np.linalg.norm(t) / len(t), 1e-12))

def fp(y):
    S = librosa.feature.mfcc(y=y, sr=SR, n_mfcc=20)
    return np.concatenate([S[1:].mean(axis=1), S[1:].std(axis=1)])

def fp_cos(f, g):
    return float(1.0 - (f @ g) / (np.linalg.norm(f) * np.linalg.norm(g)))

# "喂"，"你"，"好" —— 两组共振峰 = 两段不同文本
VOW_A = [(700, 90, 1.0), (1220, 110, 0.5), (2600, 170, 0.25)]
VOW_B = [(400, 80, 1.0), (1900, 130, 0.4), (2550, 200, 0.2)]

a1 = render(VOW_A, [0.18, 0.22, 0.20], 150, 1)
a2 = render(VOW_A, [0.19, 0.21, 0.22], 138, 2, tilt=0.6)   # 同一文本的另一次渲染
b1 = render(VOW_B, [0.20, 0.20, 0.20], 160, 3)

gapsig = np.zeros(int(SR * 0.4), dtype=np.float32)
# 修改后音频：B在前（间隔已改），A 的**重新渲染** 在后
full = np.concatenate([b1, gapsig, a2])

target = a1
t0 = len(b1) / SR + 0.4
print("音频布局: B[0, %.2f)  间隔  A2[%.2f, %.2f)" % (len(b1)/SR, t0, len(full)/SR))
print()
conf = ncc_best(target, full)
print("整句 NCC(A渲染1, 修改后音频) = %.3f" % conf)
print("  -> 高于 0.5 阈值: %s（说明这个用例不需要指纹）" % ("是" if conf >= 0.5 else "否"))

fa = fp(a1); fb = fp(b1)
W = int(SR * (len(a1) / SR * 1.2)); step = int(SR * 0.05)
best = []
for pos in range(0, max(1, len(full) - W), step):
    d_a = fp_cos(fa, fp(full[pos:pos + W]))
    d_b = fp_cos(fb, fp(full[pos:pos + W]))
    best.append((pos / SR, d_a, d_b))
best.sort(key=lambda x: x[1])
print("指纹最近的前 3 个位置 (目标=同文本渲染1):")
for p, da, db in best[:3]:
    print("   t=%.2fs  dist(同文本)=%.4f  dist(不同文本)=%.4f  margin=%.4f" % (p, da, db, db - da))
print()
b2 = sorted(best, key=lambda x: x[2])[0]
print("目标真实位置 t0=%.2f" % t0)
print("同文本最佳位置=%.2f  误差 %.2fs" % (best[0][0], abs(best[0][0] - t0)))
print("不同文本最佳位置=%.2f  误差 %.2fs" % (b2[0], abs(b2[0] - 0.0)))