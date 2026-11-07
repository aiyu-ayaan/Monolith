plugins {
    kotlin("multiplatform")
    kotlin("plugin.serialization")
    `maven-publish`
}

kotlin {
    jvm()

    sourceSets {
        commonMain.dependencies {
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.1")
            implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.0")
            implementation("org.jetbrains.kotlinx:kotlinx-datetime:0.6.2")
            api("io.ktor:ktor-client-core:3.1.0")
            implementation("io.ktor:ktor-client-content-negotiation:3.1.0")
            implementation("io.ktor:ktor-serialization-kotlinx-json:3.1.0")
            implementation("io.ktor:ktor-client-cio:3.1.0")
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.1")
            implementation("io.ktor:ktor-client-mock:3.1.0")
        }
    }
}
