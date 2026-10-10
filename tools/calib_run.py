import sys, os, json, shutil, time, tempfile; sys.path.insert(0, "src")
import numpy as np
import librosa

from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
from index_tts_gui.core.subtitler import pause_offsets
from index_tts_gui.core.speech_aligner import (
    align_sentences_detailed, cumulative_starts, recalibrate_entries_detailed,
    report_lines, CONFIDENCE_THRESHOLD, is_matched)
from index_tts_gui.core import fingerprint as FP

SRC = "projects/Fix Bug"
WS = tempfile.mkdtemp(prefix="calib_test_")
od = os.path.join(WS, "output_tts")
os.makedirs(od)
proj = json.load(open(os.path.join(SRC, "project.json"), encoding="utf-8"))
sentences = proj["sentences"]
pauses = proj["pauses"]
wav_src = collect_sentence_wavs(os.path.join(SRC, "output_tts"))
n = len(wav_src)
durations = [get_wav_duration(w) for w in wav_src]
old_starts = cumulative_starts(durations, pauses)
print("工程: %d 句, 原始音频 %.1fs" % (n, old_starts[-1] + durations[-1] + pauses[-1]))

# ── 编辑方案（模拟剪辑软件里的四种操作）──
E_INTRA_GAP   = (5, 0.8)     # 句5 内部停顿处 +0.8s
E_INTER_GAP   = (3, 1.5)     # 句3 之后的句间间隔 +1.5s
E_RESYNTH     = 200# 句200 模拟"重新合成"
E_DELETE      = 300          # 删掉句300

intra_extra = {E_INTRA_GAP[0]: E_INTRA_GAP[1]}
inter_extra = {E_INTER_GAP[0]: E_INTER_GAP[1]}

# 句200 的"重新合成"版本：变速 3% + 加噪 + 反相 -> NCC 必然失效
orig200, _sr = librosa.load(wav_src[E_RESYNTH-1], sr=None, mono=True)
resynth200 = librosa.resample(orig200, orig_sr=_sr, target_sr=int(_sr*1.03))
resynth200 = (resynth200 * 0.92 + np.random.default_rng(9).normal(0, 0.004, len(resynth200))).astype(np.float32)

# ── 构造"调整后"的音频，并同时算出每一句的真实新起点 ──
def sil(d):
    return np.zeros(max(1, int(round(d * 16000))), dtype=np.float32)

audio_parts = []
true_starts = {}
cursor_samples = 0
for i in range(1, n + 1):
    if i == E_DELETE:
        continue
    y, sr = librosa.load(wav_src[i-1], sr=16000, mono=True)
    true_starts[i] = cursor_samples / 16000.0
    audio_parts.append(y)
    cursor_samples += len(y)
    # 句内插入的间隔，插到该句内部停顿处
    if i in intra_extra:
        # 放在句子末尾附近不会命中停顿，改为在该句第一个内部停顿后插入
        pass
    p = pauses[i-1]
    if i in inter_extra:
        p += inter_extra[i]
    audio_parts.append(sil(p))
    cursor_samples += len(audio_parts[-1])

adjusted = np.concatenate(audio_parts)
print("调整后音频 %.1fs" % (len(adjusted)/16000.0))