use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=HOPP_CORE_BIN_DEFAULT");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    let is_default = env::var("HOPP_CORE_BIN_DEFAULT").unwrap_or("0".to_string()) == "1";
    let binary_name = if is_default {
        "hopp_core"
    } else {
        let target = env::var("TARGET").unwrap();
        match target.as_str() {
            "x86_64-apple-darwin" => "hopp_core-x86_64-apple-darwin",
            "aarch64-apple-darwin" => "hopp_core-aarch64-apple-darwin",
            "aarch64-pc-windows-msvc" => "hopp_core-aarch64-pc-windows-msvc",
            "x86_64-pc-windows-msvc" => "hopp_core-x86_64-pc-windows-msvc",
            "aarch64-unknown-linux-gnu" => "hopp_core-aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu" => "hopp_core-x86_64-unknown-linux-gnu",
            _ => "hopp_core",
        }
    };

    let profile = env::var("PROFILE").unwrap();
    let output_dir = if profile == "release" {
        "target/release"
    } else {
        "target/debug"
    };

    let target = env::var("TARGET").unwrap();
    if target.contains("windows") {
        // Windows uses /OUT:filename.exe
        let binary_name = format!("{binary_name}.exe");
        println!("cargo:rustc-link-arg-bin=hopp_core=/OUT:{output_dir}/{binary_name}");
    } else {
        // Unix systems use -o filename
        println!("cargo:rustc-link-arg-bin=hopp_core=-o");
        println!("cargo:rustc-link-arg-bin=hopp_core={output_dir}/{binary_name}");
    }

    // Swift bridges (apple-metal / screencapturekit) auto-link
    // libswiftCompatibility*.a. Those live under the active Xcode
    // toolchain; stale CLT paths from dependency build scripts are not
    // enough, so resolve and emit the search path here.
    if target.contains("apple-darwin") {
        link_swift_compatibility_libs();
    }
}

fn link_swift_compatibility_libs() {
    let developer_dir = env::var("DEVELOPER_DIR")
        .ok()
        .filter(|path| !path.is_empty())
        .or_else(|| {
            Command::new("xcode-select")
                .arg("-p")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        });

    let Some(developer_dir) = developer_dir else {
        println!(
            "cargo:warning=could not resolve DEVELOPER_DIR / xcode-select -p; \
             Swift compatibility libraries may fail to link"
        );
        return;
    };

    let candidates = [
        PathBuf::from(&developer_dir)
            .join("Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"),
        // Full Xcode when xcode-select still points at Command Line Tools.
        PathBuf::from("/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"),
        PathBuf::from("/Library/Developer/CommandLineTools/usr/lib/swift/macosx"),
    ];

    let Some(swift_lib_dir) = candidates.into_iter().find(|path| {
        path.join("libswiftCompatibility56.a").is_file()
            && path.join("libswiftCompatibilityConcurrency.a").is_file()
    }) else {
        println!(
            "cargo:warning=Swift compatibility libraries not found under {developer_dir}; \
             install full Xcode or set DEVELOPER_DIR"
        );
        return;
    };

    // Search path only — Swift object files already carry auto-link
    // directives for the compatibility libraries.
    println!("cargo:rustc-link-search=native={}", swift_lib_dir.display());
}
