#!/usr/bin/env bash
# В коде приложений нет вывода в журнал (задача 4.7): сгенерированные UniFFI типы
# с координатами (FriendLocation, Location) печатают их в toString/description, а
# журнал устройства читают другие инструменты. Единственное исключение —
# самопроверка для CI (ios/Staya/Core/SelfTest.swift, только в DEBUG).
set -euo pipefail
cd "$(dirname "$0")/.."

kotlin='(^|[^.[:alnum:]_])(Log\.[vdiwe]\(|Log\.wtf\(|println\(|print\(|System\.(out|err)|printStackTrace\(|Timber\.)'
swift='(^|[^.[:alnum:]_])(print\(|debugPrint\(|dump\(|NSLog\(|os_log\(|Logger\(|OSLog\()'

fail=0
if hits=$(git grep -nE "$kotlin" -- 'android/app/src/main/*.kt' 'android/core/src/main/*.kt'); then
  echo "$hits"; fail=1
fi
if hits=$(git grep -nE "$swift" -- 'ios/Staya/*.swift' ':!ios/Staya/Core/SelfTest.swift'); then
  echo "$hits"; fail=1
fi
# SelfTest — только целиком внутри #if DEBUG.
if [ "$(head -n 20 ios/Staya/Core/SelfTest.swift | grep -c '^#if DEBUG')" = 0 ]; then
  echo "ios/Staya/Core/SelfTest.swift must be wrapped in #if DEBUG"; fail=1
fi
[ "$fail" = 0 ] && echo "no log calls in app code"
exit "$fail"
