use std::{env, path::PathBuf, process::Command};

fn main() {
    #[cfg(all(target_os = "macos", feature = "desktop-pilot"))]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "open_synthetic_session",
            "get_session_state",
            "lock_vault",
        ]),
    ))
    .expect("build scoped Codeward desktop pilot");

    println!("cargo:rerun-if-changed=src/events.m");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => panic!("unsupported macOS build architecture"),
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("events.o");
    let archive = out.join("libcodeward_session_events.a");
    let mut clang = Command::new("/usr/bin/xcrun");
    clang
        .args([
            "--sdk",
            "macosx",
            "clang",
            "-arch",
            arch,
            "-fobjc-arc",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/events.m",
            "-o",
        ])
        .arg(&object);
    assert!(clang.status().expect("run Apple Clang").success());
    assert!(
        Command::new("/usr/bin/ar")
            .arg("crs")
            .arg(&archive)
            .arg(&object)
            .status()
            .expect("archive native event bridge")
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=codeward_session_events");
    for framework in ["AppKit", "Foundation", "CoreFoundation"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
