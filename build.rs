use std::process::{Command, Output};
use std::path::PathBuf;

fn setup_windows_build() {
    let absolute_path = std::fs::canonicalize("src/windows/proxy/exports.def").unwrap();
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", absolute_path.display());

    let res = tauri_winres::WindowsResource::new();
    res.compile().unwrap();
}

fn command_output_to_string(output: Output) -> String {
    String::from_utf8(output.stdout).expect("valid utf-8 from command output")
}

fn execute_command(command: &mut Command) -> Option<Output> {
    let output = command.output().ok()?;
    if !output.status.success() { return None; }
    Some(output)
}

fn setup_version_env() {
    let mut version_str = "v".to_owned() + env!("CARGO_PKG_VERSION");

    if execute_command(Command::new("git").args(["--version"])).is_some() {
        if let Some(output) = execute_command(Command::new("git").args(["rev-parse", "--short", "HEAD"])) {
            version_str.push_str("-");
            let output_str = command_output_to_string(output);
            version_str.push_str(&output_str[..output_str.len()-1]);
        }
        else {
            println!("cargo:warning=Failed to retrieve git commit hash");
        }

        if let Some(output) = execute_command(Command::new("git").args(["status", "--porcelain"])) {
            if !output.stdout.is_empty() {
                version_str.push_str("-dirty");
            }
        }
        else {
            println!("cargo:warning=Failed to retrieve git repo status");
        }

        if let Some(output) = execute_command(Command::new("git").args(["rev-parse", "--git-dir"])) {
            println!("cargo:rerun-if-changed={}", command_output_to_string(output));
        }
        else {
            println!("cargo:warning=Failed to retrieve git directory");
        }
    }
    else {
        println!("cargo:warning=Failed to execute git. Is git installed?");
    }

    println!("cargo:rustc-env=HACHIMI_DISPLAY_VERSION={}", version_str);
}

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "windows" {
        setup_windows_build();
    }

if target_os == "ios" {
        println!("cargo:rustc-link-search=native=vendor/titanox/lib");

        println!("cargo:rustc-link-lib=static=titanox");

        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");

        println!("cargo:rustc-link-lib=c++");

        println!("cargo:rerun-if-changed=vendor/titanox/include/libtitanox.h");

        let output = Command::new("xcrun")
            .args(["--sdk", "iphoneos", "--show-sdk-path"])
            .output()
            .expect("Failed to run xcrun. Is Xcode command line tools installed?");

        if !output.status.success() {
            panic!("xcrun failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let sdk_path_str = String::from_utf8_lossy(&output.stdout);
        let sdk_path = sdk_path_str.trim();

        let bindings = bindgen::Builder::default()
            .header("vendor/titanox/include/libtitanox.h")
            .clang_arg("-Ivendor/titanox/include")
            .clang_arg("-x")
            .clang_arg("objective-c++")
            .clang_arg(format!("--sysroot={}", sdk_path))
            .trust_clang_mangling(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings for Titanox");

        let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("titanox_bindings.rs"))
            .expect("Couldn't write Titanox bindings!");
    }

    setup_version_env();
}