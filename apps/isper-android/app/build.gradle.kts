import javax.inject.Inject
import org.gradle.process.ExecOperations

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// A raiz do repositório: o workspace Rust mora dois níveis acima deste projeto.
val repoRoot: File = rootProject.projectDir.resolve("../..").canonicalFile
val rustAbis: List<String> = providers.gradleProperty("isper.abis")
    .getOrElse("arm64-v8a,x86_64")
    .split(",")
    .map { it.trim() }
    .filter { it.isNotEmpty() }
val minSdkVersion = 29

// A chave que assina os APKs distribuídos (ADR 0016). Sem uma chave fixa, cada
// run do CI assina com uma chave de debug nova, e o Android só atualiza um app
// assinado pela mesma chave: instalar a versão nova exigiria desinstalar, e
// desinstalar apaga as gravações. Sem as variáveis (forks, a máquina de quem
// desenvolve), vale a chave de debug local de sempre.
val signingKeystore: String? = providers.environmentVariable("ISPER_ANDROID_KEYSTORE").orNull

android {
    namespace = "com.isper.mobile"
    // Compila contra o SDK mais novo (as bibliotecas androidx de set/2026
    // exigem o 37); o comportamento em tempo de execução segue o targetSdk.
    compileSdk {
        version = release(37) { minorApiLevel = 0 }
    }
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "com.isper.mobile"
        minSdk = minSdkVersion
        targetSdk = 36
        // Sobe a cada entrega do app (1 = laboratório da 9.1, 2 = gravador da
        // 9.2, 3 = sincronia da 9.3). O versionName é o que o PC mostra do
        // celular pareado, e o Android nunca instala um versionCode menor por
        // cima de um maior.
        versionCode = 3
        versionName = "0.3.0-sincronia"
        ndk { abiFilters += rustAbis }
    }

    signingConfigs {
        getByName("debug") {
            if (signingKeystore != null) {
                val password = providers.environmentVariable("ISPER_ANDROID_KEYSTORE_PASSWORD").get()
                storeFile = file(signingKeystore)
                storePassword = password
                keyAlias = providers.environmentVariable("ISPER_ANDROID_KEY_ALIAS").getOrElse("isper")
                keyPassword = password
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    // As .so vão descomprimidas e alinhadas no APK: o sistema as mapeia direto,
    // sem extrair (o padrão desde o Android 6, e menos espaço no aparelho).
    packaging { jniLibs { useLegacyPackaging = false } }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.play.services.code.scanner)
    // Os bindings do UniFFI carregam a libisper_mobile.so pelo JNA (a versão
    // AAR traz a libjnidispatch.so de cada ABI).
    implementation(variantOf(libs.jna) { artifactType("aar") })
    debugImplementation(libs.compose.ui.tooling)
}

// ---------------------------------------------------------------- núcleo Rust

/**
 * Compila o `isper-mobile` para cada ABI com o `cargo-ndk` e junta as .so que
 * o app precisa: a nossa, a `libc++_shared.so` do NDK (o whisper.cpp é C++) e
 * as duas do sherpa-onnx pré-compilado que o build script baixou.
 *
 * O whisper.cpp é compilado pelo CMake através do `whisper-rs-sys`, e isso
 * precisa de três ajustes para o Android:
 *
 * - um toolchain file por ABI que inclui o do NDK: sem ele o cmake-rs só
 *   declara `CMAKE_SYSTEM_NAME=Android`, e o CMake não acha o NDK;
 * - os bindings que vêm no crate (`WHISPER_DONT_GENERATE_BINDINGS`): o
 *   bindgen com o libclang do PC não acha os headers do clang para o Android,
 *   e os tipos do whisper.cpp são os mesmos em qualquer alvo de 64 bits;
 * - num PC Windows, o `whisper-rs-sys` decide pelo sistema de QUEM COMPILA:
 *   passa `/utf-8` (flag do MSVC) ao clang e pede para ligar a `advapi32`.
 *   O toolchain file tira a flag, e uma `libadvapi32.a` vazia satisfaz o
 *   link. No runner Linux do CI isso não acontece.
 */
abstract class CargoNdkBuild @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Input abstract val abis: ListProperty<String>
    @get:Input abstract val platform: Property<Int>
    @get:Input abstract val ndkDir: Property<String>
    @get:Input abstract val cmakeBinDir: Property<String>
    @get:Internal abstract val workspace: DirectoryProperty

    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val rustSources: ConfigurableFileCollection

    @get:OutputDirectory abstract val outputDir: DirectoryProperty
    @get:OutputDirectory abstract val shimDir: DirectoryProperty

    private fun triple(abi: String) = when (abi) {
        "arm64-v8a" -> "aarch64-linux-android"
        "x86_64" -> "x86_64-linux-android"
        else -> error("ABI sem alvo Rust configurado: $abi")
    }

    @TaskAction
    fun build() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        val root = workspace.get().asFile
        val ndk = File(ndkDir.get())
        val windows = System.getProperty("os.name").lowercase().contains("windows")
        val shim = shimDir.get().asFile.apply { mkdirs() }
        val slash = { f: File -> f.absolutePath.replace('\\', '/') }

        val env = mutableMapOf(
            "ANDROID_NDK_HOME" to ndk.absolutePath,
            // O .cargo/config.toml do repositório fixa flags do MSVC para o
            // desktop; no Android quem compila é o clang do NDK.
            "CMAKE_C_FLAGS_RELEASE" to "-O3 -DNDEBUG",
            "CMAKE_CXX_FLAGS_RELEASE" to "-O3 -DNDEBUG",
            // ARMv8.2 com dotprod e fp16: o que os celulares de 2019 em diante
            // têm e o que dá velocidade às matrizes quantizadas do ggml. O app
            // confere o /proc/cpuinfo antes de carregar a biblioteca.
            "GGML_CPU_ARM_ARCH" to "armv8.2-a+dotprod+fp16",
            "WHISPER_DONT_GENERATE_BINDINGS" to "1",
        )
        for (abi in abis.get()) {
            val toolchain = shim.resolve("$abi.toolchain.cmake")
            toolchain.writeText(
                """
                |# Gerado pela tarefa buildRustLibs (apps/isper-android/app/build.gradle.kts).
                |set(ANDROID_ABI $abi)
                |set(ANDROID_PLATFORM android-${platform.get()})
                |include("${slash(ndk.resolve("build/cmake/android.toolchain.cmake"))}")
                |string(REPLACE "/utf-8" "" CMAKE_CXX_FLAGS "${'$'}{CMAKE_CXX_FLAGS}")
                |string(REPLACE "/utf-8" "" CMAKE_C_FLAGS "${'$'}{CMAKE_C_FLAGS}")
                |""".trimMargin(),
            )
            val key = triple(abi).replace('-', '_')
            env["CMAKE_TOOLCHAIN_FILE_$key"] = slash(toolchain)
            if (windows) env["CARGO_TARGET_${key.uppercase()}_RUSTFLAGS"] = "-L native=${slash(shim)}"
        }
        if (windows) {
            val llvmAr = ndk.resolve("toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-ar.exe")
            exec.exec { commandLine(llvmAr.absolutePath, "rcs", shim.resolve("libadvapi32.a").absolutePath) }
            // O gerador padrão do CMake no Windows é o Visual Studio, que não
            // compila para Android; o Ninja vem com o CMake do SDK.
            env["CMAKE_GENERATOR"] = "Ninja"
            env["PATH"] = cmakeBinDir.get() + File.pathSeparator + System.getenv("PATH")
        }

        val args = mutableListOf("cargo", "ndk")
        abis.get().forEach { args += listOf("-t", it) }
        args += listOf(
            "-P", platform.get().toString(),
            "--link-libcxx-shared",
            "-o", out.absolutePath,
            "build", "--release", "-p", "isper-mobile",
        )
        exec.exec {
            workingDir = root
            commandLine(args)
            env.forEach { (k, v) -> environment(k, v) }
        }

        val prebuilt = root.resolve("target/sherpa-onnx-prebuilt")
        val needed = setOf("libsherpa-onnx-c-api.so", "libonnxruntime.so")
        for (abi in abis.get()) {
            val libs = prebuilt.walkTopDown()
                .filter { it.isFile && it.name in needed && it.parentFile.name == abi }
                .distinctBy { it.name }
                .toList()
            check(libs.size == needed.size) { "sherpa-onnx pré-compilado para $abi incompleto em $prebuilt: $libs" }
            libs.forEach { it.copyTo(out.resolve(abi).resolve(it.name), overwrite = true) }
        }
    }
}

/** Gera os bindings Kotlin do UniFFI a partir de uma libisper_mobile.so já compilada. */
abstract class UniffiBindgen @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val nativeLibs: DirectoryProperty

    @get:Input abstract val abi: Property<String>
    @get:Internal abstract val workspace: DirectoryProperty
    @get:OutputDirectory abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        val lib = nativeLibs.get().asFile.resolve("${abi.get()}/libisper_mobile.so")
        exec.exec {
            workingDir = workspace.get().asFile
            commandLine(
                "cargo", "run", "--release", "-p", "uniffi-bindgen", "--",
                "generate", "--library", lib.absolutePath,
                "--language", "kotlin", "--out-dir", out.absolutePath, "--no-format",
            )
        }
    }
}

/**
 * A reunião de exemplo do laboratório: o corpus de regressão do desktop
 * (190 s, 3 vozes, com transcrição e turnos de referência) quando existe na
 * máquina — ele não entra no Git —, senão a fala curta versionada.
 */
abstract class SpikeAssets : DefaultTask() {
    @get:Internal abstract val fixtures: DirectoryProperty
    @get:OutputDirectory abstract val outputDir: DirectoryProperty

    @TaskAction
    fun copy() {
        val out = outputDir.get().asFile.resolve("spike")
        out.deleteRecursively()
        out.mkdirs()
        val dir = fixtures.get().asFile
        val corpus = dir.resolve("reuniao-sintetica-16k.wav")
        if (corpus.isFile) {
            corpus.copyTo(out.resolve("amostra.wav"))
            dir.resolve("reuniao-sintetica.ref.txt").copyTo(out.resolve("amostra.ref.txt"))
            dir.resolve("reuniao-sintetica.turns.tsv").copyTo(out.resolve("amostra.turns.tsv"))
        } else {
            dir.resolve("fala-16k.wav").copyTo(out.resolve("amostra.wav"))
        }
    }
}

val rustSourceTree = fileTree(repoRoot.resolve("crates")) {
    include("**/*.rs", "**/Cargo.toml", "**/uniffi.toml")
}

val buildRustLibs = tasks.register<CargoNdkBuild>("buildRustLibs") {
    abis.set(rustAbis)
    platform.set(minSdkVersion)
    ndkDir.set(androidComponents.sdkComponents.ndkDirectory.map { it.asFile.absolutePath })
    cmakeBinDir.set(androidComponents.sdkComponents.sdkDirectory.map { it.asFile.resolve("cmake/3.22.1/bin").absolutePath })
    workspace.set(repoRoot)
    rustSources.from(rustSourceTree, repoRoot.resolve("Cargo.lock"), repoRoot.resolve("Cargo.toml"))
    outputDir.set(layout.buildDirectory.dir("rust/jniLibs"))
    shimDir.set(layout.buildDirectory.dir("rust/toolchains"))
}

val generateUniffiBindings = tasks.register<UniffiBindgen>("generateUniffiBindings") {
    nativeLibs.set(buildRustLibs.flatMap { it.outputDir })
    abi.set(rustAbis.first())
    workspace.set(repoRoot)
    outputDir.set(layout.buildDirectory.dir("generated/uniffi"))
}

val spikeAssets = tasks.register<SpikeAssets>("spikeAssets") {
    fixtures.set(repoRoot.resolve("fixtures"))
    outputDir.set(layout.buildDirectory.dir("generated/spikeAssets"))
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(buildRustLibs, CargoNdkBuild::outputDir)
        variant.sources.kotlin?.addGeneratedSourceDirectory(generateUniffiBindings, UniffiBindgen::outputDir)
        variant.sources.assets?.addGeneratedSourceDirectory(spikeAssets, SpikeAssets::outputDir)
    }
}
