# Local-resource macOS acceptance checkpoints

This suite opens **real Cocoa SDL windows and CoreAudio devices**. Run from a
logged-in macOS desktop with complete, legitimately obtained local resources.
It never downloads assets, installs tools, selects dummy drivers, uses AppleScript,
or requests Accessibility / Screen Recording permission.

```bash
cargo build --offline --locked --release
python3 -B tools/macos_acceptance.py \
  --data '/absolute/path/to/data' \
  --output '/absolute/path/to/new-acceptance-evidence'
```

The output directory must be new. The runner accepts `--binary`, `--movie`,
`--audio`, `--timeout` (default 120 seconds per checkpoint), and `--movie-seconds`
(default 12). Choose a movie with its own audio and visible movement in the
selected initial interval; an all-black opening, silent movie or insufficient
interval fails rather than being silently accepted. Media overrides must resolve
inside the selected resource tree. Explicit SDL overrides other than Cocoa /
CoreAudio are rejected, not replaced. Other environment/font/art settings are
recorded and preserved. The runner supplies an isolated windowed, nonmuted INI.

## Evidence and pass criteria

| Checkpoint | What actually executes | Required evidence |
| --- | --- | --- |
| Title | Normal title loader, menu composition, Chinese labels, title BGM | Required title artwork decoded; nonuniform SDL renderer capture; Chinese characters and zero logged missing glyphs; decoded PCM and queued audio bytes |
| IC / CC / Coda | Normal game interpreter and renderer, starting at 1002 / 2001 / 3002 | At least two completed dialogue waits, real background, Chinese text, decoded source scripts, renderer capture |
| Save/load in each scene | Actual runtime snapshot → native save file → loader → normal runtime reconstruction → next script wait | Byte-identical re-encoding; restored scene/PC; exact before/after rendered pixels; subsequent script PC/scene advancement |
| Movie | Engine Movie decoder, paced SDL presentation, embedded audio through Mixer | At least two frames and changing frame hashes, nonuniform SDL capture, decoded PCM, CoreAudio output queue |
| Fresh-process load for IC / CC / Coda | Separate process loads the prior real save, reconstructs scene, continues interpreter | Restored screenshot matches prior process exactly; real SDL capture; scene/PC advances |
| Music / voice | Separate existing bounded `audio-test` calls with local BGM and voice OGG | CoreAudio, positive decoded samples and queued bytes |

Every engine checkpoint writes `report.txt` ending in `status=pass` only after
all its requirements succeed. The runner requires this marker, native driver
names, image sizes/payloads and save comparisons; exit zero alone is insufficient.
Per-checkpoint stdout/stderr logs and TGA captures accompany `summary.json`.
The summary includes SHA-256 and sizes for the native binary, selected real
scripts, fonts, movie and audio input. Before launch it rejects an installation
marker and verifies every tree's filtered file count and bytes against the local
release manifest, including AMP-only filters for original backgrounds. This
inventory check is offline; ZIP SHA-256 validation remains in the downloader
log. Input hashes identify the particular resource-backed run; neither these nor
count/byte totals alone establish asset provenance/authenticity.

`renderer.tga` is captured from SDL's actual renderer before presentation, at
its drawable size. Other TGAs are engine surfaces. Neither is a macOS compositor
screenshot or proof the window was unobscured. No OS screenshot permission is
needed. Positive PCM/queue evidence proves decode/device submission, **not**
that speakers were unmuted, the listener heard audio, or A/V synchronization was
perceptually correct. Visually review Chinese glyphs and ordinary scene captures
and listen manually before claiming those subjective properties.

Each scene first checks an on-disk save and fresh interpreter/scene reconstruction
in the same process, then a **separate process** loads that save and compares its
restored image with the original process before continuing. This tests persistence
across process exit/restart, but does not test the interactive save/load UI.
Coda uses 3002's ordinary farewell-toast scene, verified from its decoded script;
3001's adult opening is deliberately excluded from automated captures.
The selected ordinary checkpoint in each section is covered, not route choices,
endings, full chapters, a full playthrough, or adult scenes. Diagnostic saves go
only into the evidence directory; story read-history writes are suppressed in
acceptance mode. Normal artwork/font caches may still be created.

The engine has an independent watchdog, including blocking decoder/startup work.
The runner adds an outer deadline and kills the isolated process group, including
leftover ffmpeg children, on completion/failure/timeout. Timed-out checkpoints
remain failed and partial evidence stays available. Prefer the runner over direct
commands because a watchdog exit alone cannot reap external decoder descendants.

## Engine contract

```text
wa2-native acceptance-title OUT
wa2-native acceptance-scene {1002|2001|3002} OUT
wa2-native acceptance-load SAVE OUT
wa2-native acceptance-movie FILE OUT SECONDS
```

Set `WA2_DATA`, `WA2_LANG=cn` and optionally `WA2_ACCEPTANCE_TIMEOUT=120`.
Native commands default to a 120-second deadline, bounded to 5–600 seconds.
Missing resources produce nonzero exit / blocked runner status, never a pass.
No successful actual-resource execution is implied by adding these facilities.

## Additional opt-in diagnostics

```bash
# Native fullscreen/restore + square/wide resize + Retina pixel/pointer checks.
./target/release/wa2-native acceptance-display /tmp/wa2-display-new

# Use an input shorter than the test duration so a real loop boundary is crossed.
WA2_AUDIO_TEST_SE=/path/to/data/se/SE_8410.OGG \
WA2_AUDIO_TEST_VOICE=/path/to/data/voice/1002_0000_10.OGG \
  ./target/release/wa2-native audio-continuity /path/to/data/bgm/BGM_001_B.OGG 120

# Fresh directories, isolated INI: capture actual options and save/load rendering.
WA2_INI=/tmp/wa2-diagnostic.ini WA2_OPTIONS_CAPTURE=/tmp/wa2-options-new \
  ./target/release/wa2-native acceptance-title /tmp/wa2-title-new
WA2_INI=/tmp/wa2-diagnostic.ini WA2_SAVELOAD_CAPTURE=/tmp/wa2-saveload-new \
  ./target/release/wa2-native acceptance-scene 1002 /tmp/wa2-scene-new
```

Capture hooks return without processing interactive save actions. Save/load
captures include both visual modes and first/last pages, reading existing slots
without modifying them; a supplied scene checkpoint appears in a separately
labeled in-memory preview, never in a user slot. These hooks test rendering, not
interactive clicking or overwrite confirmation. They preserve the existing
100 slot IDs despite the reconstructed two-column/five-row presentation.

Audio continuity measures queued-depth/decoder counters, overlapping media,
main-thread stalls and a real loop boundary. It does not record physical device
output or certify subjective sound quality. Original PC cursor/hover-sound
mapping, every UI animation, all movie remasters and full-route fidelity remain
outside the demonstrated scope. Generated reports contain local paths and game
captures: retain them locally, not in source commits.

## Runner unit tests (not gameplay)

```bash
python3 -B -m unittest discover -s tools -p test_macos_acceptance.py -v
```

These use small synthetic temporary files and mocked subprocesses to verify
fail-closed report validation, native-driver rejection, resource path containment,
image truncation detection, hashing and failed-process cleanup. They are not
resource-backed acceptance results.
