use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_ADMIN");
    if env::var_os("CARGO_FEATURE_ADMIN").is_none() {
        return;
    }

    println!("cargo:rerun-if-changed=admin-ui/index.html");
    println!("cargo:rerun-if-changed=admin-ui/package.json");
    println!("cargo:rerun-if-changed=admin-ui/tsconfig.json");
    println!("cargo:rerun-if-changed=admin-ui/vite.config.ts");
    println!("cargo:rerun-if-changed=admin-ui/src");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .args(["--prefix", "admin-ui", "run", "build"])
        .current_dir(&manifest_dir)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to launch {npm} for admin-ui build: {error}. Install Node.js and run 'npm --prefix admin-ui install' if dependencies are missing"
            )
        });

    if !status.success() {
        panic!(
            "admin-ui production build failed. Run 'npm --prefix admin-ui install' and 'npm --prefix admin-ui run build' to inspect the failure"
        );
    }
}