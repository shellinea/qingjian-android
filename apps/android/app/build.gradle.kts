import org.apache.tools.ant.taskdefs.condition.Os

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "im.qingjian"
    compileSdk = 35

    defaultConfig {
        applicationId = "im.qingjian"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // 随包数据：词库 / 语言模型 / 释义表，未压缩，安装时原样落到 assets
    androidResources {
        noCompress += listOf("qj", "tsv", "qjm")
    }

    sourceSets {
        getByName("main") {
            // cargo 编出来的 .so 由 cargoBuild 任务拷到这里。
            // 声明 srcDir 时 Gradle 会检查到 mergeDebugJniLibFolders 读的是 cargoBuild 的
            // 输出却无显式依赖，必须补上（否则构建结果取决于任务执行顺序）。
            jniLibs.srcDir("src/main/jniLibs")
        }
    }
}

// 编 APK 前先有 .so；也满足 AGP 的隐式依赖检查
tasks.matching {
    it.name.startsWith("assemble") || it.name == "mergeDebugNativeLibs" || it.name.contains("JniLibFolders")
}.configureEach { dependsOn(cargoTask) }

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
}

/** 用 cargo 编两个 ABI 的 cdylib，产物拷进 jniLibs。 */
val cargoTask = tasks.register("cargoBuild") {
    description = "Build Rust cdylib for Android ABIs"
    group = "build"

    val abis = listOf("arm64-v8a", "x86_64")
    val crateDir = rootProject.projectDir.resolve("crate")
    val jniLibsDir = projectDir.resolve("src/main/jniLibs")

    outputs.dir(jniLibsDir)

    doLast {
        val cargoHome = System.getenv("CARGO_HOME") ?: "D:\\DevTools\\rust\\.cargo"
        val cargo = "$cargoHome\\bin\\cargo.exe"
        val ndk = System.getenv("ANDROID_NDK_HOME") ?: "D:\\Android\\Sdk\\ndk\\30.0.16248370"
        val prebuilt = File(ndk, "toolchains/llvm/prebuilt/windows-x86_64/bin").absolutePath
        val api = 24

        // build script 是 host 上的 x86_64-pc-windows-msvc 程序，链接要 MSVC 的 link.exe。
        // 环境里没有（例如从干净终端起 Gradle）时补上 VS Build Tools 的默认位置。
        val msvcBin = File("C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC")
            .walk()
            .filter { it.isDirectory && it.name == "hostx64" }
            .map { File(it, "x64").absolutePath }
            .firstOrNull()
        val hostPath = listOfNotNull(msvcBin).joinToString(";")

        for (abi in abis) {
            val target = if (abi == "arm64-v8a") "aarch64-linux-android" else "x86_64-linux-android"
            // CC 给 C 依赖（aws-lc-sys 等）用：带 API 级别的 .cmd，NDK 脚本包装，
            // 等价 clang --target=<triple> $*。build.rs 自己拼 target 时认它。
            // LINKER 给 rustc 用：直调 clang.exe，靠 CFLAGS_* 里的 --target / --sysroot 生效。
            // 二者分工：.cmd 只在 cc 链路被验证可用（C 依赖已全过）；rustc 起 linker 是直
            // 接的进程调用，不经过 cmd.exe，用 .cmd 会 "not recognized as an internal or
            // external command"。
            val clang = File(prebuilt, "$target-$api-clang.cmd").absolutePath
            val linker = File(prebuilt, "clang.exe").absolutePath
            val cmakeFile = File(ndk, "build/cmake/android.toolchain.cmake").absolutePath
            val sysroot = "$prebuilt/../sysroot"

            val env = mapOf(
                "CC_$target" to clang,
                "CXX_$target" to File(prebuilt, "clang++.exe").absolutePath,
                "AR_$target" to File(prebuilt, "llvm-ar.exe").absolutePath,
                "RANLIB_$target" to File(prebuilt, "llvm-ranlib.exe").absolutePath,
                "CARGO_TARGET_${target.uppercase().replace('-', '_')}_LINKER" to linker,
                "CMAKE_TOOLCHAIN_FILE" to cmakeFile,
                "ANDROID_ABI" to abi,
                "ANDROID_PLATFORM" to "android-$api",
                "ANDROID_NDK" to ndk,
                // rustc 直调 clang.exe 时不会自己带 target，要靠 RUSTFLAGS 的 link-arg 传给链接器；
                // 这会让依赖按新 flags 重编一次，之后 cargo 缓存命中，只此一次。
                "RUSTFLAGS" to "-C link-arg=--target=$target$api -C link-arg=--sysroot=$sysroot",
                "CFLAGS_$target" to "--target=$target$api --sysroot=$sysroot",
                "CXXFLAGS_$target" to "--target=$target$api --sysroot=$sysroot",
            )

            logger.lifecycle("cargo build $target ...")
            val command = listOf(
                cargo, "build", "-p", "qingjian-android",
                "--target", target, "--release",
            )
            // 不 clear：build script 是 host（x86_64-pc-windows-msvc）程序，要用 MSVC 的 link.exe。
            // 在这个进程的环境上叠加交叉编译变量即可。
            val process = ProcessBuilder(command)
                .directory(crateDir)
                .redirectErrorStream(true)
                .apply {
                    env.forEach { (k, v) -> environment()[k] = v }
                }
                .start()
            process.inputStream.bufferedReader().forEachLine { logger.lifecycle("  $it") }
            val code = process.waitFor()
            check(code == 0) { "cargo build for $target failed with $code" }

            // 仓库是 cargo workspace，target 目录在 workspace 根（本 crate 上级的
            // 三级），不在 crate 目录下。
            val workspaceRoot = crateDir.parentFile.parentFile.parentFile
            val so = workspaceRoot.resolve("target/$target/release/libqingjian_android.so")
            check(so.exists()) { "找不到产物: $so" }
            val destDir = File(jniLibsDir, abi).apply { mkdirs() }
            so.copyTo(File(destDir, "libqingjian_android.so"), overwrite = true)
            logger.lifecycle("  -> ${so.name} copied to $abi")
        }
    }
}
