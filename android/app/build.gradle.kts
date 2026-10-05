plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "io.github.realfamousbae.staya"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.realfamousbae.staya"
        minSdk = 29
        targetSdk = 37
        // Релиз (scripts/release.sh) передаёт версию из тега: -PstayaVersionName=0.2.0 -PstayaVersionCode=2.
        versionCode = providers.gradleProperty("stayaVersionCode").orElse("1").get().toInt()
        versionName = providers.gradleProperty("stayaVersionName").orElse("0.1.0").get()

        // Rust-ядро собирается только под arm64 (см. scripts/build-android-core.sh).
        ndk { abiFilters += "arm64-v8a" }

        // Тесты на эмуляторе в CI (Keystore настоящий только на устройстве).
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    // Ключ подписи релиза — только на Mac автора (scripts/release-key.sh), путь и
    // пароли — из окружения сборки (scripts/release.sh берёт пароль из связки ключей
    // macOS). Без них релиз собирается неподписанным: CI и lint не нужен ключ.
    val keystore = providers.environmentVariable("STAYA_KEYSTORE").orNull
    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = providers.environmentVariable("STAYA_KEYSTORE_PASSWORD").get()
                // Свой псевдоним — только для проверки релиза в CI отладочным ключом.
                keyAlias = providers.environmentVariable("STAYA_KEY_ALIAS").orElse("staya").get()
                keyPassword = providers.environmentVariable("STAYA_KEYSTORE_PASSWORD").get()
            }
        }
    }

    buildTypes {
        release {
            if (keystore != null) signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    buildFeatures {
        compose = true
    }

    // Никаких метаданных о зависимостях в APK: они нужны только Google Play.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }
}

dependencies {
    implementation(project(":core"))
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui.tooling.preview)
    debugImplementation(libs.androidx.compose.ui.tooling)
    // HTTP и WebSocket к серверу Staya (задача 4.1b): проверка ключа сервера при
    // установке соединения, пул соединений, WebSocket. Без Google Play Services.
    implementation(libs.okhttp)
    // QR-коды приглашений (задача 4.3): ZXing core — чистая Java, без Google Play
    // Services; кодирование и распознавание кадров камеры.
    implementation(libs.zxing.core)
    // Камера для сканера QR: CameraX (AndroidX, без GMS).
    implementation(libs.camerax.camera2)
    implementation(libs.camerax.lifecycle)
    implementation(libs.camerax.view)
    // Карта (задача 4.4): MapLibre Native — открытый движок векторных карт без
    // Google Play Services и без телеметрии. Стиль, тайлы, шрифты и спрайты — только
    // с сервера Staya; все запросы карты идут через наш OkHttp с проверкой ключа.
    implementation(libs.maplibre)

    testImplementation(libs.junit)
    // JVM-тесты сетевого слоя: настоящий TLS (MockWebServer + тестовые сертификаты)
    // и настоящее ядро через JNA, как в :core.
    testImplementation(libs.okhttp.mockwebserver)
    testImplementation(libs.okhttp.tls)
    testImplementation(libs.jna)
    // Только для тестов на эмуляторе: AndroidJUnitRunner и AndroidJUnit4.
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
}

// JVM-тесты с настоящим ядром: собранная под хост библиотека из :core (buildHostCore).
val repoRoot = rootProject.layout.projectDirectory.dir("..")
tasks.withType<Test>().configureEach {
    dependsOn(":core:buildHostCore")
    systemProperty("jna.library.path", repoRoot.dir("target/debug").asFile.absolutePath)
}
