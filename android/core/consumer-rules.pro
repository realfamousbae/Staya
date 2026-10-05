# Правила R8 для приложения, которое подключает :core (релизная сборка с минификацией).
# UniFFI-привязки вызывают Rust через JNA, а JNA находит классы, поля структур и
# колбэки по именам через рефлексию: переименование или удаление ломает вызовы
# в рантайме, а не при сборке.
-dontwarn java.awt.**
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class uniffi.** { *; }
