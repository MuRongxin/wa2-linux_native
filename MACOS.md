# Experimental macOS local-source launch

**This remains an experimental source-build path, not an upstream supported or
signed macOS release.** On macOS 15.5 ARM64, resource-backed title, Chinese dialogue,
IC1002 / CC2001 / Coda3002 progression, exact save/restore rendering, fresh-process
reload, real movie/BGM/voice checkpoints passed. Native high-DPI rendering,
fullscreen/window restoration and arbitrary-aspect resizing were exercised too.
Full-route gameplay, all movies, subjective audio sync and every interactive
control are not certified. See [checkpoint documentation](<tools/MACOS_ACCEPTANCE.md>)
to reproduce the checks and generate local evidence. Game assets, captures, saves
and machine-specific logs are intentionally not included in this repository.

Use [run-macos.sh](<run-macos.sh>), **not** the existing Linux
[run.sh](<run.sh>). The macOS launcher never executes or downloads a Linux
AppImage, installs packages, or fetches game resources. The Linux launch and
installation scripts are unchanged.

## Follow-up: audio continuity and Retina/UI fidelity

The first startup checkpoints did **not** establish continuous sound or faithful
UI composition. Follow-up fixes replace upstream sleep-per-block audio scheduling
with queued-depth feedback and bounded prebuffering; restore original translucent
option sheets/state artwork over the current scene; restore the title label atlas;
and enable SDL high-DPI windows with drawable-pixel rendering and logical mouse
coordinates. Native options keep source-resolution pixels rather than downsampling
to 720p first. Known corrupt Chinese button/corner regions fall back narrowly to
original Japanese/shared UI pixels without modifying resources.

Measured 120-second CoreAudio stress crossed a real BGM loop with overlapping
voice/SE and 2-second main-thread stalls, zero observed decoder/queue starvation,
and ~48,000 submitted frames/second. This is not a listening or hardware loopback
certification. Reproduce with the continuity diagnostic below; retain its stdout
with the tested binary hash. Queue-empty observations are not hardware underrun
counters. UI feedback uses a separate replaceable slot to avoid stacking hover
sounds without stopping scenario sound effects.

Diagnostic commands (use fresh output directories and isolated `WA2_INI`):

```bash
./target/release/wa2-native audio-continuity data/bgm/BGM_001_B.OGG 120
./target/release/wa2-native acceptance-display /tmp/wa2-display-new
WA2_INI=/tmp/wa2-isolated.ini WA2_OPTIONS_CAPTURE=/tmp/wa2-options-new \
  ./target/release/wa2-native acceptance-scene 1002 /tmp/wa2-scene-new
```

Quit the old running game after saving before relaunching: compiled fixes do not
hot-update its process. Original PC animation/layout equivalence, every screen,
all routes and subjective audio sync remain outside the verified scope.

## Case-sensitive resource storage (important on default macOS APFS)

This resource release contains different files named `T0010`/`t0010` and
`T0100`/`t0100` in its scaled background trees. Default case-insensitive APFS
cannot represent them separately. **Do not ignore a resource inventory mismatch
or rename one arbitrarily.** Use a case-sensitive APFS volume or sparsebundle.
The downloader now checks ZIP filename collisions before extraction.

Optional convenience layout: place a case-sensitive image named
`WA2Data.sparsebundle` next to the checkout, with its `data` folder exposed at
`../wa2-data-volume/data` and the checkout's `data` symlink pointing there.
Normal [run-macos.sh](<run-macos.sh>) launches can mount this already-created
image if the link is dangling. The launcher does not create or populate images.
`--check`, `--build-only` and `--smoke` never mount it. Explicit `WA2_DATA` always
wins, so an existing case-sensitive external volume is equally valid. Finder
users can double-click [启动WA2.command](<启动WA2.command>).

This is an ordinary user-mounted disk image, not an OS partition change; it grows
as resources/caches are written. Do not detach it while the game runs.

## Commands

From this checkout (quote its path if it contains spaces):

```bash
cd "/path/to/wa2-native"
./run-macos.sh --check
./run-macos.sh --build-only
./run-macos.sh --smoke
WA2_DATA="$HOME/Games/WA2 data" ./run-macos.sh
WA2_DATA="$HOME/Games/WA2 data" ./run-macos.sh 1002
```

| Mode | Behavior |
| --- | --- |
| `--check` | Offline, read-only checks of local build tools, libraries, media tools, fonts and data layout. No downloads, installs, builds, window, caches or Python bytecode writes. Missing requirements return nonzero. |
| `--build-only` | Checks build-tool paths, then compiles the release binary. Does not require SDL2, FreeType, ffmpeg or game data; these are loaded/used at runtime. |
| `--smoke` | Checks build tools and SDL2, compiles, then runs the existing `fmtinfo` diagnostic. Briefly opens a small SDL window and prints driver/pixel-format information. Does not need game data, FreeType or ffmpeg. |
| Default / `title` | Checks dependencies and local data **before** compilation, then builds and opens the title screen. |
| Numeric scene | Same checks/build, then executes the source `run SCENE` command. |

Builds use `cargo build` / `cargo run` with `--offline --locked --release`. Cargo
may write build artifacts, but cannot fetch crate dependencies. The current
[Cargo.toml](<Cargo.toml>) has no external crate dependencies. Install the Rust
toolchain yourself first: Cargo's offline flag does not configure or install
Rust. Existing `CARGO_TARGET_DIR`, Cargo configuration, `RUSTUP_TOOLCHAIN`,
`RUSTFLAGS` and other explicit build settings are left alone. Cross-compilation
settings can produce a binary that this Mac cannot run; use a native toolchain
and compatible library architecture.

## Dependencies: check first, install only what is missing

The launcher needs Python 3.8+, installed Rust/Cargo 1.90+, and Apple's C linker /
Xcode Command Line Tools. Normal gameplay also needs SDL2, FreeType, `ffmpeg` and
`ffprobe`. `dwebp` is optional; ffmpeg is the fallback WebP decoder.

The checker prints found paths and loadable libraries, and suggests Homebrew
formulas only for missing requirements. It does **not** assume that every package
needs installation. For reference, the corresponding manual commands are:

```bash
# Examples only: choose missing components, do NOT blindly run all of these.
brew install python
brew install rust                 # or use your already installed rustup toolchain
xcode-select --install            # only if Apple command-line tools are missing
brew install sdl2 freetype ffmpeg  # install only missing runtime components
brew install webp                 # optional dwebp decoder
```

No Homebrew command or shell profile is run by the launcher. Add your existing
Homebrew/Rust installation to `PATH` yourself if needed. The read-only check
locates tool executables without running compiler/version shims: it cannot prove
that an installed-looking rustup shim has a usable toolchain, or that `cc` has an
active SDK. A build will report those errors. The build-only mode intentionally
does not demand runtime dependencies.

The checker and runtime try macOS dylib names, Apple Silicon Homebrew locations
under `/opt/homebrew`, Intel Homebrew locations under `/usr/local`, and SDL2's
framework under `/Library/Frameworks`. Python, Rust and the libraries should use
the same native architecture. An installed library can still fail to load because
of an architecture mismatch or a missing transitive library; that is not proof
that its formula must be reinstalled. Custom prefixes may require user-managed
dynamic-loader configuration. No `DYLD_*` variables are replaced by this launcher.

## Local data and fonts

`WA2_DATA` defaults to the checkout's `data` directory. Relative `WA2_DATA` and
font paths are interpreted relative to the checkout, even when the launcher is
called from another directory. Explicit values are passed through unchanged:
use `WA2_DATA="$HOME/Games/WA2 data"`, not a quoted literal `~/Games/WA2 data`.

The checker reuses only `runtime_ready()` from
[fetch_data.py](<launcher/fetch_data.py#L329-L347>), without running the downloader.
It requires the initial script pair, nonempty audio/movie/bitmap-font directories,
and one matching art-tree scale. Chinese-script presence also requires its initial
script pair and matching Chinese art. An interrupted-install marker makes the
data incomplete. This is a **layout check**, not a complete inventory, asset hash,
font-glyph or scene validation. Explicit `WA2_LANG` is checked for validity too.
The vector fonts shipped under [fonts](<fonts/manifest.tsv>) do not replace the
bitmap-font assets required inside the external game data.

Font discovery remains in the source engine. It uses the bundled font manifest
and native macOS font locations; the launcher does not replace that logic or copy
system fonts. If you need a manual native-font selection, use **only environment
overrides**, for example:

```bash
# Use an installed CJK font; availability varies by macOS version.
WA2_FONT='/System/Library/Fonts/Hiragino Sans GB.ttc' ./run-macos.sh
# An explicit face index is also supported:
WA2_FONT='/absolute/path/to/My CJK font.ttc#0' ./run-macos.sh
```

`WA2_FONT` and role-specific `WA2_FONT_HOME`, `WA2_FONT_HISTORY`, `WA2_FONT_GAME`
settings, including explicit empty values, are preserved. The source validates
font faces/coverage and may reject an unsuitable override. No fonts are installed
or downloaded by the check.

## SDL diagnostic and limits

Run `--smoke` in an interactive macOS desktop session. Inspect the reported video
driver (normally `cocoa`) and pixel formats, not just the exit status. The diagnostic
only exercises SDL initialization/window/texture setup and format names; it is
**not a gameplay, font, audio, movie or sustained rendering test**.

The launcher never silently selects `SDL_VIDEODRIVER=dummy`,
`SDL_AUDIODRIVER=dummy`, software rendering, or an X11 display. Explicit user
`SDL_*` and `WA2_SOFTWARE` settings are retained and relevant driver overrides are
shown by the checker. A dummy-driver test therefore requires your deliberate
configuration and is not evidence that the native desktop works. No automatic
fallback is added on failure.

Normal gameplay can write saves, settings and caches beneath `WA2_DATA`; this is
separate from the read-only `--check` operation. Quit the game normally to finish
a manual launch test. Do not infer a complete game port from a successful launch.

## Checks performed for this change

The launcher regression tests are self-contained and use temporary fixture data
and mocked command-line tools, not game assets:

```bash
bash -n run-macos.sh
python3 -B -m unittest discover -s launcher -p test_macos.py -v
```

They cover dependency failure before compilation, check/build/smoke routing,
explicit environment preservation (including empty values), paths containing
spaces, native-platform rejection, argument validation, targeted install advice,
and the existing local-data predicate. Synthetic fixtures are not playable game
data. Resource-backed Rust tests use the shared `WA2_DATA`/checkout data locator
rather than hard-coded developer paths. The complete Rust suite **requires real
assets**, including Chinese 2x/4x/8x UI variants, plus native font/media libraries;
it is not a data-free CI suite. Missing data fails instead of silently skipping.

Validation on macOS 15.5 ARM64 included 261 Rust tests with complete resources,
38 Python tests, ten native runtime checkpoints, a 120-second real-media audio
continuity stress and native resize/fullscreen checks. These are bounded checks,
not full-route gameplay certification. Linux execution and Intel macOS were not
verified for this patch. Shared audio/rendering/UI changes need Linux review too.

```bash
WA2_DATA=/path/to/complete/data cargo test --offline --locked --release -- --test-threads=1
python3 -B -m unittest discover -s launcher -p 'test_*.py' -v
python3 -B -m unittest discover -s tools -p test_macos_acceptance.py -v
```

## External resource release: opt-in only

The repository's external
[data-v1.1.0 release](<https://github.com/MuRongxin/wa2-linux_native/releases/tag/data-v1.1.0>)
contains the game data as 29 independent ZIP parts. Resources are not part of
this source change and are not fetched by the macOS launcher. The existing
[README](<README.md#L54-L58>) lists approximately 45.63 GB compressed, 50.79 GB
extracted, and roughly 100 GB of free space during installation. Check the actual
release and free disk space before opting in. Selecting 2x art in the game does
not make the existing downloader fetch a smaller subset.

If you deliberately want the full resource download, the existing Python
standard-library downloader runs on macOS (`fcntl` is available) and does not
fetch or execute an AppImage. This release requires a **case-sensitive destination**
as explained above. Optional `--jobs 4` enables bounded parallel downloads; default
is serial. Run it **separately**, from this checkout:

```bash
# OPTIONAL, LARGE DOWNLOAD: this writes data and download/extraction caches.
# --proxy '' opts into direct GitHub only, rather than the downloader's proxies.
python3 -B launcher/fetch_data.py \
  --repo MuRongxin/wa2-linux_native --tag data-v1.1.0 \
  --dest "$PWD" --data-dir "${WA2_DATA-$PWD/data}" \
  --proxy '' --ensure --remove-parts

# Afterwards, check locally and launch with run-macos.sh, not run.sh.
./run-macos.sh --check
```

Only use data you are entitled to obtain and use. This opt-in operation downloads,
verifies and extracts the full release, retaining resumable cache on interruption.
The downloader's `--dry-run` still contacts GitHub/fetches metadata, and
`--check-only` can extract cached resources: **neither is used by the macOS
read-only check**. If downloader messages suggest rerunning the Linux launcher,
repeat the explicit Python command above instead.
