import java.util.Base64
import java.net.URI
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest

plugins {
    id("com.android.application")
}

val gatewayUrl = providers.gradleProperty("AIOS_SERVER_URL")
    .orElse("http://10.0.2.2:8787")
    .get()

val sherpaOnnxVersion = "1.13.4"
val sherpaOnnxSha256 = "03f9c4df965f21c71269365a7951a7f23b5696fddd093fa318c80d65550ab780"
val sherpaOnnxAar = layout.buildDirectory.file("dependencies/sherpa-onnx-$sherpaOnnxVersion.aar")

val downloadSherpaOnnx by tasks.registering {
    outputs.file(sherpaOnnxAar)
    doLast {
        val target = sherpaOnnxAar.get().asFile
        fun sha256(file: File): String = MessageDigest.getInstance("SHA-256")
            .digest(file.readBytes()).joinToString("") { "%02x".format(it) }

        if (target.exists() && sha256(target) == sherpaOnnxSha256) return@doLast
        target.parentFile.mkdirs()
        val temporary = File(target.parentFile, "${target.name}.part")
        URI(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$sherpaOnnxVersion/" +
                "sherpa-onnx-$sherpaOnnxVersion.aar"
        ).toURL().openStream().use { input -> temporary.outputStream().use(input::copyTo) }
        check(sha256(temporary) == sherpaOnnxSha256) { "sherpa-onnx AAR checksum mismatch" }
        Files.move(temporary.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
    }
}

android {
    namespace = "dev.voiceos.client"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.voiceos.client"
        minSdk = 31
        targetSdk = 36
        versionCode = 4
        versionName = "0.4.0"

        ndk {
            // VoiceOS currently targets the Pixel and other modern ARM64 Android devices.
            abiFilters += "arm64-v8a"
        }

        buildConfigField("String", "GATEWAY_BASE_URL", "\"$gatewayUrl\"")
    }

    buildFeatures {
        buildConfig = true
    }

    buildTypes {
        debug {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
        }
        release {
            manifestPlaceholders["usesCleartextTraffic"] = "false"
            isMinifyEnabled = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets.getByName("main").res.srcDir(layout.buildDirectory.dir("generated/vic-sound/res").get().asFile)
}

val generateVicSound by tasks.registering {
    val source = layout.projectDirectory.file("src/main/vic_checkin.wav.b64")
    val output = layout.buildDirectory.file("generated/vic-sound/res/raw/vic_checkin.wav")
    inputs.file(source)
    outputs.file(output)
    doLast {
        val target = output.get().asFile
        target.parentFile.mkdirs()
        target.writeBytes(Base64.getMimeDecoder().decode(source.asFile.readText()))
    }
}

tasks.named("preBuild").configure {
    dependsOn(generateVicSound)
    dependsOn(downloadSherpaOnnx)
}

dependencies {
    implementation(files(sherpaOnnxAar).builtBy(downloadSherpaOnnx))
    testImplementation("junit:junit:4.13.2")
}
