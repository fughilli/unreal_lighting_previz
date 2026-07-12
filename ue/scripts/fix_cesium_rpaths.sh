#!/usr/bin/env bash
# Fix Cesium for Unreal prebuilt binaries for an INSTALLED engine on macOS.
#
# Cesium's GitHub binary release (v2.28.0, UE 5.8) ships dylibs whose rpaths are
# relative and assume the plugin lives inside the engine tree. With an installed
# engine at /Users/Shared/Epic Games/UE_5.8 and the project elsewhere, those
# relative rpaths resolve to /UE_5.8/... (filesystem root) and Cesium can't find
# its sibling engine-plugin deps (SunPosition, Niagara, Water) → "module
# 'CesiumRuntime' could not be loaded". This adds absolute rpaths to the real
# engine plugin dirs and re-signs (adhoc) so the deps resolve.
#
# Prereqs: SunPosition + Water plugins enabled in the .uproject (Cesium deps).
# Re-run this after any Cesium reinstall/update.
set -euo pipefail

ENG="${UE_ENGINE:-/Users/Shared/Epic Games/UE_5.8/Engine}"
PROJ_PLUGINS="${1:-/Users/kevin/Documents/Unreal Projects/PrevizTest/Plugins}"
CESDIR="$PROJ_PLUGINS/CesiumForUnreal/Binaries/Mac"

RPATHS=(
  "$ENG/Binaries/Mac"
  "$ENG/Plugins/Runtime/SunPosition/Binaries/Mac"
  "$ENG/Plugins/FX/Niagara/Binaries/Mac"
  "$ENG/Plugins/Experimental/Water/Binaries/Mac"
)

for dyl in libUnrealEditor-CesiumRuntime.dylib libUnrealEditor-CesiumEditor.dylib; do
  echo "patching $dyl"
  for rp in "${RPATHS[@]}"; do
    # -add_rpath fails if the rpath already exists; ignore that.
    install_name_tool -add_rpath "$rp" "$CESDIR/$dyl" 2>/dev/null || true
  done
  codesign -f -s - "$CESDIR/$dyl"
done
echo "done — Cesium dylibs patched + re-signed"
