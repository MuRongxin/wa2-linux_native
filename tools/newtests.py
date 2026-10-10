

def test_slice_map_uses_absolute_origin():
    """回归：切片边界是**句内相对偏移**，map 收到的是**全音频绝对时间**。

    句 3 的原起点是 200.0s、整句位移 +2.0s。旧实现拿绝对时间 200.0
    去和句内相对边界 [0.0, 2.0, 5.0] 做二分，必然落到最后一个切片，
    于是映射结果凭空多出整个句子的位移（实测第 54 句 200.01s 被映射成
    402.16s）。旧测试全部用 old_starts=[0.0]，正好绕开了这个 bug，
    所以必须显式钉住"非零起点"这一情形。
    """
    from index_tts_gui.core.speech_aligner import (
        AlignResult, CONFIDENCE_THRESHOLD, SentenceTimeMap,
        recalibrate_entries_detailed,
    )

    origin = 200.0
    tm = SentenceTimeMap(5.0, [2.0], [2.0, 2.0], 2.0, origin)
    assert tm.slice_of(200.0) == 0
    assert tm.map(200.0) == 202.0
    assert tm.slice_of(202.5) == 1
    assert tm.map(202.5) == 204.5

    result = AlignResult([202.0], [CONFIDENCE_THRESHOLD], [[2.0, 2.0]])
    out, report = recalibrate_entries_detailed(
        [_mk_entry(200.0, 205.0)], [origin], [5.0], result.starts,
        [[2.0]], result.slice_deltas,
    )
    assert out[0].start_sec == 202.0, f"起点应映射到 202.0，实际 {out[0].start_sec}"
    assert out[0].end_sec == 207.0, f"终点应映射到 207.0，实际 {out[0].end_sec}"
    assert not report.has_issues


def test_slice_deltas_are_relative_not_absolute(tmp_path):
    """回归：切片位移是相对原位置的位移，不是绝对位置。

    _locate_one_sentence_slices 在全音频时间轴上匹配（返回绝对秒数），
    而切片模板的 t_start 是句内相对偏移。少减一次 origin，第 54 句的
    切片位移就会是 202.15 而不是 2.15。
    """
    from index_tts_gui.core.speech_aligner import _locate_all_slice_deltas

    sr = 8000

    def _voice(f0, dur, seed):
        rng = np.random.default_rng(seed)
        t = np.arange(int(sr * dur)) / sr
        x = sum(a * np.sin(2 * np.pi * f0 * k * t + rng.uniform(0, 6.28))
                for k, a in ((1, 1.0), (2, .5), (3, .33)))
        return (0.7 * x + rng.normal(0, .01, len(x))).astype(np.float32)

    head, tail = _voice(135, 0.9, 1), _voice(210, 0.8, 2)
    mid = np.concatenate([head, np.zeros(int(sr * 0.3), np.float32), tail])

    p = tmp_path / "sentence_01_a.wav"
    with wave.open(str(p), "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
        w.writeframes(b"".join(
            struct.pack("<h", int(v * 32000)) for v in np.clip(mid, -1, 1)))

    lead = int(sr * 1.5)
    full = np.concatenate([
        np.zeros(lead, np.float32), mid, np.zeros(int(sr * 1.5), np.float32)])
    old_starts = [1.5]

    deltas = _locate_all_slice_deltas(
        full, sr, [str(p)], [[0.9]], [1.5], [0.99], len(full) / sr, old_starts,
    )
    assert deltas[0], "应当定位到切片位移"
    for k, d in enumerate(deltas[0]):
        assert d is not None, f"切片 {k} 应当被定位到"
        assert abs(d) < 0.5, (
            f"切片 {k} 位移应是相对量（内容没动 -> ≈0），实际 {d:.3f}"
        )
