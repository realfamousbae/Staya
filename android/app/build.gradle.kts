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
        versionCode = 1
        versionName = "0.1.0"

        // Rust-ядро собирается только под arm64 (см. scripts/build-android-core.sh).
        ndk { abiFilters += "arm64-v8a" }

        // Тесты на эмуляторе в CI (Keystore настоящий только на устройстве).
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
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
