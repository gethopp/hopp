use std::env;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        build_windows_camera();
    }
    println!("cargo:rerun-if-env-changed=HOPP_CORE_BIN_DEFAULT");
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
}

fn build_windows_camera() {
    println!("cargo:rerun-if-changed=src/camera/windows_capture.cpp");
    println!("cargo:rerun-if-changed=src/camera/windows_capture.h");
    println!("cargo:rerun-if-env-changed=LK_CUSTOM_WEBRTC");
    println!("cargo:rerun-if-env-changed=LK_DEBUG_WEBRTC");

    let webrtc_dir = webrtc_sys_build::webrtc_dir();
    if !webrtc_dir.exists() {
        webrtc_sys_build::download_webrtc()
            .expect("Failed to download WebRTC camera capture library");
    }
    let include = webrtc_dir.join("include");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("src/camera/windows_capture.cpp")
        .include(&include)
        .include(include.join("third_party/abseil-cpp"))
        .include(include.join("third_party/libyuv/include"))
        .flag("/std:c++20")
        .flag("/EHsc");
    for (name, value) in webrtc_sys_build::webrtc_defines() {
        build.define(&name, value.as_deref());
    }
    build.compile("hopp_windows_camera");
}
