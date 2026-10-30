#!/usr/bin/env bash
# tools/ios/inject_ipa.sh
# Inject libhachimi.dylib into an IPA for sideloading (TrollStore / Sideloadly)
#
# Usage:
#   ./tools/ios/inject_ipa.sh <input.ipa> <libhachimi.dylib> [output.ipa] [options]
#
# Options:
#   --display-name "My App"   Custom name shown on iOS home screen
#   --files-sharing            Enable Files app access to Documents/
#
# Requirements (install via brew):
#   brew install ldid
#   brew install optool   (or build from: https://github.com/alexzielenski/optool)
#
# On Windows: run this via GitHub Actions macOS runner or WSL with Homebrew.
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AOT_PATCHER="$SCRIPT_DIR/aot_patcher.py"

INPUT_IPA="${1:?Usage: $0 <input.ipa> <libhachimi.dylib> [output.ipa] [--hooks-json FILE] [--display-name NAME] [--files-sharing]}"
DYLIB="${2:?Usage: $0 <input.ipa> <libhachimi.dylib> [output.ipa]}"

if [[ "${3:-}" == --* || -z "${3:-}" ]]; then
    OUTPUT_IPA="$(dirname "$INPUT_IPA")/$(basename "${INPUT_IPA%.ipa}")_hachimi.ipa"
    shift 2 2>/dev/null || true
else
    OUTPUT_IPA="$3"
    shift 3 2>/dev/null || true
fi

# ── Parse extra options ──────────────────────────────────────────
DISPLAY_NAME=""
FILES_SHARING=false
HOOKS_JSON=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --display-name) DISPLAY_NAME="$2"; shift 2 ;;
        --files-sharing) FILES_SHARING=true; shift ;;
        --hooks-json)   HOOKS_JSON="$2"; shift 2 ;;
        *) shift ;;
    esac
done

if [[ -z "$HOOKS_JSON" ]]; then
    if [[ -f "hooks.json" ]]; then
        HOOKS_JSON="hooks.json"
    elif [[ -f "$SCRIPT_DIR/hooks.json" ]]; then
        HOOKS_JSON="$SCRIPT_DIR/hooks.json"
    fi
fi

# ── Entitlements for re-signing ───────────────────────────────────
ENTITLEMENTS_PLIST="$(mktemp /tmp/hachimi_entitlements.XXXXXX.plist)"
cat > "$ENTITLEMENTS_PLIST" <<'PLIST_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>get-task-allow</key><true/>
</dict>
</plist>
PLIST_EOF

# ── Check dependencies ────────────────────────────────────────────
check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo "Error: '$1' not found. Install with: brew install $2"
        exit 1
    fi
}
check_cmd zip zip
check_cmd unzip unzip
check_cmd ldid ldid
check_cmd python3 python3
# optool is optional; fallback to insert_dylib if available
USE_OPTOOL=false
USE_INSERT_DYLIB=false
if command -v optool &>/dev/null; then USE_OPTOOL=true; fi
if command -v insert_dylib &>/dev/null; then USE_INSERT_DYLIB=true; fi
if ! $USE_OPTOOL && ! $USE_INSERT_DYLIB; then
    echo "Error: Need 'optool' or 'insert_dylib' to inject LC_LOAD_DYLIB."
    echo "  optool:       brew install optool  (or build from github.com/alexzielenski/optool)"
    echo "  insert_dylib: brew install insert_dylib (or github.com/Tyilo/insert_dylib)"
    exit 1
fi

# ── Workspace ─────────────────────────────────────────────────────
WORK_DIR="$(mktemp -d /tmp/hachimi_inject.XXXXXX)"
trap "rm -rf '$WORK_DIR' '$ENTITLEMENTS_PLIST'" EXIT

echo "[inject] Extracting IPA..."
unzip -q "$INPUT_IPA" -d "$WORK_DIR"

# Locate the .app bundle
APP_BUNDLE=$(find "$WORK_DIR/Payload" -maxdepth 1 -name "*.app" | head -1)
if [[ -z "$APP_BUNDLE" ]]; then
    echo "Error: No .app bundle found in Payload/"
    exit 1
fi
APP_NAME=$(basename "$APP_BUNDLE" .app)
APP_EXEC="$APP_BUNDLE/$APP_NAME"
echo "[inject] App bundle : $APP_BUNDLE"

UNITY_FW="$APP_BUNDLE/Frameworks/UnityFramework.framework/UnityFramework"
if [[ -f "$UNITY_FW" ]]; then
    if [[ -n "$HOOKS_JSON" && -f "$HOOKS_JSON" ]]; then
        echo "[inject] Applying AOT JIT-less patches using $HOOKS_JSON..."
        python3 "$AOT_PATCHER" "$UNITY_FW" "$HOOKS_JSON" "$UNITY_FW.tmp"
        mv "$UNITY_FW.tmp" "$UNITY_FW"
        chmod 755 "$UNITY_FW"
        echo "[inject] UnityFramework successfully patched."
    else
        echo "[inject] WARN: hooks.json not found. Skipping AOT binary patching."
    fi
else
    echo "[inject] WARN: UnityFramework not found at $UNITY_FW"
fi

# ── Copy dylib into Frameworks ────────────────────────────────────
mkdir -p "$APP_BUNDLE/Frameworks"
cp "$DYLIB" "$APP_BUNDLE/Frameworks/libhachimi.dylib"
echo "[inject] Copied dylib to Frameworks/"

# ── Strip __RESTRICT segment (prevents DYLD_INSERT_LIBRARIES from being ignored) ──
if $USE_OPTOOL; then
    echo "[inject] Stripping __RESTRICT segment..."
    optool strip -t __RESTRICT -s __restrict -i "$APP_EXEC" 2>/dev/null || true
fi

# ── Inject LC_LOAD_DYLIB load command ─────────────────────────────
echo "[inject] Injecting LC_LOAD_DYLIB..."
LOAD_PATH="@rpath/libhachimi.dylib"
if $USE_OPTOOL; then
    optool install -c load -p "$LOAD_PATH" -t "$APP_EXEC"
elif $USE_INSERT_DYLIB; then
    insert_dylib --strip-codesig --inplace "$LOAD_PATH" "$APP_EXEC"
fi

# ── Fake-sign dylib with ldid ─────────────────────────────────────
echo "[inject] Fake-signing dylib..."
ldid -S "$APP_BUNDLE/Frameworks/libhachimi.dylib"

if [[ -f "$UNITY_FW" ]]; then
    ldid -S "$UNITY_FW"
fi

# ── Re-sign the entire .app with entitlements ─────────────────────
# Note: for TrollStore, fake-sign is enough. For Sideloadly, use real cert below.
echo "[inject] Fake-signing app binary (TrollStore/jailbreak mode)..."
ldid -S"$ENTITLEMENTS_PLIST" "$APP_EXEC"

# Optional: uncomment to use real code signing (Sideloadly with developer cert)
# CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-iPhone Developer}"
# codesign --force --sign "$CODESIGN_IDENTITY" \
#   --entitlements "$ENTITLEMENTS_PLIST" \
#   "$APP_BUNDLE/Frameworks/libhachimi.dylib"
# codesign --force --sign "$CODESIGN_IDENTITY" \
#   --entitlements "$ENTITLEMENTS_PLIST" \
#   "$APP_BUNDLE"

# ── Repack IPA ────────────────────────────────────────────────────
echo "[inject] Repacking IPA..."

# ── Patch Info.plist (display name + Files sharing) ───────────────
PLIST="$APP_BUNDLE/Info.plist"
plutil -convert xml1 "$PLIST" 2>/dev/null || true

if [[ -n "$DISPLAY_NAME" ]]; then
    /usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName $DISPLAY_NAME" "$PLIST" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string $DISPLAY_NAME" "$PLIST"
    echo "[inject] CFBundleDisplayName → $DISPLAY_NAME"
fi

if $FILES_SHARING; then
    /usr/libexec/PlistBuddy -c "Set :UIFileSharingEnabled true" "$PLIST" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Add :UIFileSharingEnabled bool true" "$PLIST"
    /usr/libexec/PlistBuddy -c "Set :LSSupportsOpeningDocumentsInPlace true" "$PLIST" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Add :LSSupportsOpeningDocumentsInPlace bool true" "$PLIST"
    echo "[inject] Files app sharing → enabled"
fi

mkdir -p "$(dirname "$OUTPUT_IPA")"
ABS_OUTPUT_IPA="$(cd "$(dirname "$OUTPUT_IPA")" 2>/dev/null && pwd)/$(basename "$OUTPUT_IPA")"
pushd "$WORK_DIR" > /dev/null
zip -qr "$ABS_OUTPUT_IPA" Payload
popd > /dev/null

echo ""
echo "[inject] Done!"
echo "  Output IPA : $OUTPUT_IPA"
echo "  Size       : $(du -sh "$OUTPUT_IPA" | cut -f1)"
echo ""
echo "Install options:"
echo "  TrollStore  : AirDrop or copy to device, open with TrollStore"
echo "  Sideloadly  : drag-drop the IPA into Sideloadly"
echo "  LiveContainer: copy libhachimi.dylib directly to Tweaks/ folder instead"
