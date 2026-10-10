import sys
sys.path.insert(0, "/home/lrxin/文档/indexx/src")
from index_tts_gui.core import subtitler, splitter

print(repr(splitter.RuleBasedSplitter(max_length=0).split("你好
世界。再见")))
s = splitter.RuleBasedSplitter(max_length=10)
text = "这是一段很长的中文文本因为今天天气不错所以我们决定出门散步并且顺便买点东西回来"
print("pause cut:", s._find_pause_word_cut(text, 20), repr(text[:20]))
print(s.split(text))
g1 = splitter.word_cut_guard(text)
g2 = splitter.word_cut_guard(text)
print("memoized:", g1 is g2, "cache:", len(splitter._WORD_CUT_CACHE))

orig_detect = subtitler._detect_pauses
subtitler._detect_pauses = lambda p: []
sentence = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十"
for dur in (0.0, 0.4, 1.0, 2.0, 3.5, 10.0):
    ents, nxt = subtitler._build_entries_for_sentence(
        sentence, dur, "fake.wav", 5.0, 5.0 + dur, 8, 1,
    )
    bad = [e for e in ents if e.end_sec < e.start_sec]
    print("dur=%.2f n=%d last_end=%.4f want=%.4f neg=%d"
          % (dur, len(ents), ents[-1].end_sec, 5.0 + dur, len(bad)))
    assert not bad
    assert all(e.end_sec <= 5.0 + dur + 1e-9 for e in ents)
subtitler._detect_pauses = orig_detect

from index_tts_gui.core.io_ass import _seconds_to_ass_time
print(_seconds_to_ass_time(1.996), _seconds_to_ass_time(3661.125), _seconds_to_ass_time(0.0), _seconds_to_ass_time(59.999))

from index_tts_gui.core.speech_aligner import cumulative_starts, is_matched
print(cumulative_starts([1.0, 2.0, 3.0], [0.5]))
print(cumulative_starts([1.0, 2.0, 3.0]))
print(cumulative_starts([1.0, 2.0], [0.5, 0.25, 9.9]))
print(is_matched(1.0, 0.6), is_matched(-1.0, 0.9), is_matched(1.0, 0.4))
print("OK")
