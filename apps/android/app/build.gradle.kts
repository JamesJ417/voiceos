import java.util.Base64

plugins {
    id("com.android.application")
}

val gatewayUrl = providers.gradleProperty("AIOS_SERVER_URL")
    .orElse("http://10.0.2.2:8787")
    .get()

android {
    namespace = "dev.voiceos.client"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.voiceos.client"
        minSdk = 31
        targetSdk = 36
        versionCode = 19
        versionName = "0.14.0"

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

tasks.named("preBuild").configure { dependsOn(generateVicSound) }

dependencies {
    testImplementation("junit:junit:4.13.2")
}
