

def test_silent_sentence_is_reported_missing_not_stacked(tmp_path):
    """回归：近乎静音的分句必须判缺失，不能堆到全文的同一个任意峰上。

    真实项目（Fix Bug，546 句）里有 5 句时长0.01~0.03s、RMS=3e-6 的
    分句——TTS 把"3""6"这类数字读成了几乎无声的片段。这类信号去均值后
    只剩数值噪声，任意两个静音模板之间的 NCC 都能接近 1.0，于是它们会
    一起堆到同一个峰上：实测句506 与句518 被同时校准到 352.45s（真实
    位置分别是 2029.37 / 2056.25，误差上千秒）。

    字幕被放到完全无关的位置比报缺失糟糕得多，所以这里要求判缺失。
    """
    import numpy as np
    import soundfile as sf
    from index_tts_gui.core.speech_aligner import align_sentences_detailed

    sr = 8000
    rng = np.random.default_rng(11)

    def _voice(f0, dur, seed):
        r = np.random.default_rng(seed)
        t = np.arange(int(sr * dur)) / sr
        x = sum(a * np.sin(2 * np.pi * f0 * k * t + r.uniform(0, 6.28))
                for k, a in ((1, 1.0), (2, .5), (3, .33)))
        return (0.7 * x + r.normal(0, .01, len(x))).astype(np.float32)

    # 两段真实语音，中间夹两段近乎静音的"句子"
    real_a, real_b = _voice(150, 1.0, 1), _voice(230, 1.0, 2)
    quiet = (rng.normal(0, 3e-6, int(sr * 0.03))).astype(np.float32)

    p_real = tmp_path / "sentence_01_a.wav"
    p_quiet1 = tmp_path / "sentence_02_q1.wav"
    p_quiet2 = tmp_path / "sentence_03_q2.wav"
    p_real2 = tmp_path / "sentence_04_b.wav"
    for path, x in ((p_real, real_a), (p_quiet1, quiet),
                    (p_quiet2, quiet), (p_real2, real_b)):
        sf.write(str(path), x, sr)

    fullp = tmp_path / "full.wav"
    gap = np.zeros(int(sr * 0.3), dtype=np.float32)
    sf.write(str(fullp), np.concatenate([
        real_a, gap, quiet, gap, quiet, gap, real_b]), sr)

    res = align_sentences_detailed(
        str(fullp),
        [str(p_real), str(p_quiet1), str(p_quiet2), str(p_real2)],
        ["A", "3", "6", "B"], [0.3, 0.3, 0.3, 0.0],
    )
    assert res.starts[0] >= 0, "正常语音不该受影响"
    assert res.starts[3] >= 0, "正常语音不该受影响"
    for i in (1, 2):
        assert res.starts[i] < 0, (
            f"静音句 {i+1} 必须判缺失，实际被放到 {res.starts[i]:.2f}"
        )
    # 更关键：两个静音句不能被放到同一个位置
    if res.starts[1] >= 0 or res.starts[2] >= 0:
        raise AssertionError("静音句不应有位置")
