

def test_align_sentences_honors_should_cancel(tmp_path):
    """回归：should_cancel 能真的中断对齐，而不是跑完才返回。

    长项目（546 句 / 35 分钟音频）上校准要跑半小时，界面上按"停止"
    必须能立刻生效。原实现没有任何中断点，只能等全部跑完。
    """
    from index_tts_gui.core.speech_aligner import align_sentences_detailed

    sr = 8000
    rng = np.random.default_rng(5)

    def _voice(f0, dur, seed):
        r = np.random.default_rng(seed)
        t = np.arange(int(sr * dur)) / sr
        x = sum(a * np.sin(2 * np.pi * f0 * k * t + r.uniform(0, 6.28))
                for k, a in ((1, 1.0), (2, .5)))
        return (0.7 * x + r.normal(0, .01, len(x))).astype(np.float32)

    wavs, gaps = [], []
    for i in range(8):
        y = _voice(140 + 15 * i, 0.5, i)
        p = tmp_path / f"sentence_{i+1:02d}.wav"
        sf.write(str(p), y, sr)
        wavs.append(str(p))
        gaps.append(np.zeros(int(sr * 0.2), dtype=np.float32))

    fullp = tmp_path / "full.wav"
    sf.write(str(fullp), np.concatenate(
        [x for i in range(8) for x in (wavs and [_voice(140 + 15 * i, 0.5, i)], gaps[i])]),
        sr)

    calls = {"n": 0}

    def _cancel() -> bool:
        calls["n"] += 1
        return calls["n"] > 3   # 第 4 次检查时放弃

    res = align_sentences_detailed(
        str(fullp), wavs, [str(i) for i in range(8)],
        [0.2] * 8, None, None, should_cancel=_cancel,
    )
    assert res.starts == [], "取消后不应返回任何定位结果"
    assert calls["n"] > 3, "取消回调应当被多次调用"


def test_align_sentences_reports_progress(tmp_path):
    """回归：对齐过程要上报进度，否则长项目在界面上看起来像卡死。"""
    from index_tts_gui.core.speech_aligner import align_sentences_detailed

    sr = 8000

    def _voice(f0, dur, seed):
        r = np.random.default_rng(seed)
        t = np.arange(int(sr * dur)) / sr
        x = sum(a * np.sin(2 * np.pi * f0 * k * t + r.uniform(0, 6.28))
                for k, a in ((1, 1.0), (2, .5)))
        return (0.7 * x + r.normal(0, .01, len(x))).astype(np.float32)

    wavs, parts = [], []
    for i in range(6):
        y = _voice(150 + 20 * i, 0.5, i)
        p = tmp_path / f"sentence_{i+1:02d}.wav"
        sf.write(str(p), y, sr)
        wavs.append(str(p))
        parts.append(y)
        parts.append(np.zeros(int(sr * 0.2), dtype=np.float32))

    fullp = tmp_path / "full.wav"
    sf.write(str(fullp), np.concatenate(parts), sr)

    seen = []
    align_sentences_detailed(
        str(fullp), wavs, [str(i) for i in range(6)], [0.2] * 6,
        on_progress=lambda d, t, s: seen.append((d, t, s)),
    )
    assert seen, "必须至少上报一次进度"
    assert all(t == 6 for _, t, _ in seen), "total 应为句子总数"
    assert seen[-1][0] == 6, "最后一次应报告完成"
    stages = {s for _, _, s in seen}
    assert any("粗匹配定位" in s for s in stages), stages
    assert [d for d, _, _ in seen] == sorted(d for d, _, _ in seen), "进度应单调递增"
