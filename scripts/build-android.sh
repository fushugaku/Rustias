#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if ! command -v node >/dev/null 2>&1; then
  printf 'Node.js 22 or newer is required.\n' >&2
  exit 1
fi
if [[ -z "${JAVA_HOME:-}" ]] && [[ -d /opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home ]]; then
  export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
fi
if [[ -z "${ANDROID_HOME:-}" ]] && [[ -z "${ANDROID_SDK_ROOT:-}" ]] && [[ -d /opt/homebrew/share/android-commandlinetools/platforms ]]; then
  export ANDROID_HOME=/opt/homebrew/share/android-commandlinetools
fi
android/gradlew -p android assembleDebug testDebugUnitTest lintDebug "$@"
node scripts/verify-android.mjs
mkdir -p output/android
cp android/app/build/outputs/apk/debug/app-debug.apk output/android/Rustias-debug.apk
printf 'APK ready: %s/output/android/Rustias-debug.apk\n' "$PWD"
