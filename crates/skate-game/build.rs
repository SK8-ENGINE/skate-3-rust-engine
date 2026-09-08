use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../docs/images/skating-crab.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let sdk = PathBuf::from(env::var_os("ProgramFiles(x86)").expect("Windows SDK location"))
        .join("Windows Kits/10/bin");
    let rc = env::var_os("RC").map(PathBuf::from).or_else(|| {
        let mut paths: Vec<_> = fs::read_dir(sdk).ok()?.filter_map(Result::ok)
            .map(|entry| entry.path().join("x64/rc.exe"))
            .filter(|path| path.is_file()).collect();
        paths.sort();
        paths.pop()
    }).expect("Install the Windows SDK resource compiler or set RC to rc.exe");
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../docs/images/skating-crab.ico").canonicalize().unwrap();
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let script = output.join("skate3rust.rc");
    let resource = output.join("skate3rust.res");
    fs::write(&script, format!("1 ICON \"{}\"\n", icon.display().to_string().replace('\\', "/"))).unwrap();
    assert!(Command::new(rc).arg("/nologo").arg("/fo").arg(&resource).arg(script)
        .status().expect("Run Windows resource compiler").success(), "Icon compilation failed");
    println!("cargo:rustc-link-arg-bin=skate3rust={}", resource.display());
}
