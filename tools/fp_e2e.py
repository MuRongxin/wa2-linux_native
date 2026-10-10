import sys, os, tempfile, wave, struct; sys.path.insert(0, "src")
import numpy as np
from index_tts_gui.core import fingerprint as FP
from index_tts_gui.core.speech_aligner import align_sentences_detailed

SR = FP.FINGERPRINT_SR

def render(formants, syl, f0_base, seed):
    rng = np.random.default_rng(seed)
    out = []
    for dur in syl:
        n = int(SR*dur); t = np.arange(n)/SR
        x = np.zeros(n)
        for (fr, bw, amp) in formants:
            x += amp*np.exp(-np.pi*bw*t)*np.cos(2*np.pi*fr*t + rng.uniform(0, 6.28))
        f0 = f0_base*(1 + 0.05*np.sin(2*np.pi*1.5*t + rng.uniform(0, 6.28)))
        x *= (0.6 + 0.4*np.sin(2*np.pi*np.cumsum(f0)/SR))
        x *= (0.5 + 0.5*np.sin(np.pi*t/dur))
        out.append(x)
    return (np.concatenate(out)+rng.normal(0,0.003,int(SR*sum(syl)))).astype(np.float32)

def write(path, y):
    y = np.clip(y, -1, 1)
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(v*32000)) for v in y))

def sil(d): return np.zeros(int(SR*d), dtype=np.float32)

VOW_A = [(700, 90, 1.0), (1220, 110, 0.5), (2600, 170, 0.25)]
VOW_B = [(400, 80, 1.0), (1900, 130, 0.4), (2550, 200, 0.2)]

d = tempfile.mkdtemp()
a1 = render(VOW_A, [0.20,0.22,0.20], 150, 1)
a2 = render(VOW_A, [0.21,0.215,0.205], 137, 2)   # 重新渲染
b1 = render(VOW_B, [0.21,0.21,0.21], 158, 3)

# 磁盘上的 take 是"重新渲染后"的 a2 —— 模拟用户增量合成
p1 = os.path.join(d, "sentence_01_a.wav"); write(p1, a2)
p2 = os.path.join(d, "sentence_02_b.wav"); write(p2, b1)

# full_dub.wav 里是旧的 a1（合并时的版本），句2 被移到了句1 前面并改了间隔
full = np.concatenate([b1, sil(0.4), a1])
fp_path = os.path.join(d, "full.wav"); write(fp_path, full)

# 指纹在合并时落盘：描述的是 a1（full_dub.wav 里的那一版）
fp1 = FP.compute(a1, SR)      # 注意：不是 a2
FP.save(d, {1: fp1, 2: FP.compute(b1, SR)})
print("full_dub.wav 布局: B[0,%.2f) 间隔 A1[%.2f,%.2f)" % (len(b1)/SR, len(b1)/SR+0.4, len(full)/SR))
print("磁盘 take = a2（重新渲染版），full_dub.wav = a1（合并时的版本）")
print()

r_no = align_sentences_detailed(fp_path, [p1,p2], ["A","B"], [0.4, 0.0], None, None)
print("--- 无指纹（纯波形匹配）---")
print("  starts:", [round(v,2) for v in r_no.starts], "scores:", [round(v,2) for v in r_no.scores])

loaded = FP.load(d)
r_fp = align_sentences_detailed(fp_path, [p1,p2], ["A","B"], [0.4,0.0], None, loaded)
print("--- 有指纹 ---")
print("  starts:", [round(v,2) for v in r_fp.starts], "scores:", [round(v,2) for v in r_fp.scores])
print()
print("句1 真实位置 = %.2f" % (len(b1)/SR + 0.4))
ok = abs(r_fp.starts[0] - (len(b1)/SR+0.4)) < 0.2 and r_no.starts[0] < 0
print("结果: %s" % ("指纹救回了重新合成的句子" if ok else "未达预期"))