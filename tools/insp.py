import json, os
from index_tts_gui.core.merger import collect_sentence_wavs, get_wav_duration
SRC="projects/Fix Bug"
proj=json.load(open(os.path.join(SRC,"project.json"),encoding="utf-8"))
sents=proj["sentences"]
wavs=collect_sentence_wavs(os.path.join(SRC,"output_tts"))
for i in (78,121,506,518,300,200):
    print("句%-4d 时长%.2fs  %s"%(i,get_wav_duration(wavs[i-1]),sents[i-1][:60]))
print()
for a,b in ((78,121),(506,518)):
    print("句%d vs 句%d 文本相同? %s"%(a,b,sents[a-1]==sents[b-1]))
