// Rust-ядро Staya: .so и Kotlin-привязки собирает scripts/build-android-core.sh.
plugins {
    alias(libs.plugins.android.library)
}

val rustOut = layout.buildDirectory.dir("rust")
val rustProfile = providers.gradleProperty("rustProfile").orElse("release")

val buildRustCore by tasks.registering(Exec::class) {
    group = "build"
    description = "Собирает Rust-ядро и генерирует Kotlin-привязки"
    val script = rootProject.layout.projectDirectory.file("../scripts/build-android-core.sh")
    commandLine(script.asFile.absolutePath, rustProfile.get())
    // Перезапускаем при изменении Rust-кода.
    inputs.dir(rootProject.layout.projectDirectory.dir("../core/src"))
    inputs.dir(rootProject.layout.projectDirectory.dir("../proto/src"))
    inputs.file(rootProject.layout.projectDirectory.file("../Cargo.lock"))
    inputs.file(rootProject.layout.projectDirectory.file("../core/uniffi.toml"))
    inputs.file(script)
    inputs.property("profile", rustProfile)
    outputs.dir(rustOut)
}

android {
    namespace = "io.github.realfamousbae.staya.core"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        minSdk = 29
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    testOptions {
        // В JVM-тестах android.os.Build — заглушка: SDK_INT = 0, привязки берут JNA-очистку.
        unitTests.isReturnDefaultValues = true
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(rustOut.get().dir("jniLibs").asFile.path)
            kotlin.directories.add(rustOut.get().dir("kotlin").asFile.path)
        }
    }
}

tasks.named("preBuild") { dependsOn(buildRustCore) }

// JVM-тесты привязок: ядро собирается под сам компьютер (macOS/Linux) и грузится через JNA.
val repoRoot = rootProject.layout.projectDirectory.dir("..")
val buildHostCore by tasks.registering(Exec::class) {
    group = "verification"
    description = "Собирает Rust-ядро под хост для JVM-тестов"
    workingDir(repoRoot)
    commandLine("cargo", "build", "-p", "staya-core", "--lib")
}

tasks.withType<Test>().configureEach {
    dependsOn(buildHostCore)
    systemProperty("jna.library.path", repoRoot.dir("target/debug").asFile.absolutePath)
}

dependencies {
    // Нужна Kotlin-привязкам UniFFI для вызова нативной библиотеки.
    implementation(libs.jna) { artifact { type = "aar" } }
    // @RequiresApi в привязках UniFFI для Android (core/uniffi.toml).
    implementation(libs.androidx.annotation)

    // Обычный jar JNA содержит нативный диспетчер для macOS/Linux — нужен для JVM-тестов.
    testImplementation(libs.jna)
    testImplementation(libs.junit)
}
