#!/bin/zsh
# Build 白色相簿2.app. Usage: macos/bundle.sh <data dir> <out .app>
set -euo pipefail
DATA=$1; APP=$2
SRC=${0:A:h:h}
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
(cd $SRC && cargo build --release)
C=$APP/Contents
mkdir -p $C/MacOS $C/Frameworks $C/Resources
swiftc -O -target arm64-apple-macos13 $SRC/macos/Launcher.swift -o $C/MacOS/WA2
cp $SRC/target/release/wa2-native $C/MacOS/wa2-native
for t in ffmpeg ffprobe dwebp; do cp -L /opt/homebrew/bin/$t $C/MacOS/$t; chmod u+w $C/MacOS/$t; done
cp -L /opt/homebrew/lib/libSDL2-2.0.0.dylib /opt/homebrew/lib/libfreetype.6.dylib $C/Frameworks/
cp -L /opt/homebrew/lib/libSDL3.0.dylib $C/Frameworks/libSDL3.dylib
chmod u+w $C/Frameworks/*.dylib
# copy + rewrite all non-system dylib deps
python3 -I $SRC/macos/bundle_libs.py $C/Frameworks $C/MacOS/ffmpeg $C/MacOS/ffprobe $C/MacOS/dwebp \
  $C/Frameworks/libSDL2-2.0.0.dylib $C/Frameworks/libfreetype.6.dylib $C/Frameworks/libSDL3.dylib
install_name_tool -id @loader_path/libSDL3.dylib $C/Frameworks/libSDL3.dylib
rsync -a --delete $SRC/fonts/ $C/Resources/fonts/
if [[ $DATA != - ]]; then rsync -a --delete --exclude cache --exclude save --exclude wa2.ini $DATA/ $C/Resources/data/; fi
# App icon, rendered at build time from the player's own data tree (the
# repository ships no game art). Skipped when the picture is missing.
ICON_SRC=${DATA:-}/bg_4x/T0000.webp
[[ $DATA == - ]] && ICON_SRC=$C/Resources/data/bg_4x/T0000.webp
if [[ -f $ICON_SRC ]]; then
  T=$(mktemp -d)
  swiftc -O $SRC/macos/icon/MakeIcon.swift -o $T/makeicon
  dwebp -quiet $ICON_SRC -o $T/bg.png
  $T/makeicon $T/bg.png $T/icon.png
  mkdir $T/AppIcon.iconset
  for s in 16 32 128 256 512; do
    sips -z $s $s $T/icon.png --out $T/AppIcon.iconset/icon_${s}x${s}.png >/dev/null
    sips -z $((s*2)) $((s*2)) $T/icon.png --out $T/AppIcon.iconset/icon_${s}x${s}@2x.png >/dev/null
  done
  iconutil -c icns $T/AppIcon.iconset -o $C/Resources/AppIcon.icns
  rm -rf $T
fi
cat > $C/Info.plist <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>WA2</string>
<key>CFBundleIdentifier</key><string>local.wa2native.whitealbum2</string>
<key>CFBundleName</key><string>白色相簿2</string>
<key>CFBundleDisplayName</key><string>白色相簿2</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>1.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>LSApplicationCategoryType</key><string>public.app-category.role-playing-games</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSArchitecturePriority</key><array><string>arm64</string></array>
<key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
</dict></plist>
PL
# sign inner code, then the bundle
for f in $C/Frameworks/*.dylib $C/MacOS/ffmpeg $C/MacOS/ffprobe $C/MacOS/dwebp $C/MacOS/wa2-native; do
  codesign -f -s - $f 2>/dev/null
done
codesign -f -s - $APP
codesign --verify --deep --strict $APP && echo SIGNED
