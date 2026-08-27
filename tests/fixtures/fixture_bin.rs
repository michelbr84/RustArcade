//! Single-file test fixture compiled with `rustc` at test time.
//!
//! * `--version`            → prints a cargo-like version line
//! * `install --root D ...` → behaves like `cargo install`: writes a runnable copy of
//!   itself to `D/bin/<crate>` and a `.crates2.json`. Behaviour is driven by the crate
//!   name (`fixture-fail` fails, `fixture-slow` sleeps) and by an optional
//!   `fixture-version` file next to this executable.
//! * anything else          → behaves like a game: `--exit N`, `--sleep-ms N`,
//!   `--print TEXT`, `--pwd`, `--echo-env NAME`, `--crash`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("cargo 1.97.1 (fixture)"),
        Some("install") => fake_cargo_install(&args[1..]),
        _ => fake_game(&args),
    }
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
}

fn fake_cargo_install(args: &[String]) {
    let mut root: Option<PathBuf> = None;
    let mut bins: Vec<String> = Vec::new();
    let mut krate: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                root = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--bin" => {
                if let Some(b) = args.get(i + 1) {
                    bins.push(b.clone());
                }
                i += 2;
            }
            "--color" | "--version" | "--features" => i += 2,
            a if a.starts_with("--") => i += 1,
            a => {
                krate = Some(a.to_string());
                i += 1;
            }
        }
    }
    let root = root.unwrap_or_else(|| {
        eprintln!("error: --root is required");
        process::exit(2);
    });
    let krate = krate.unwrap_or_else(|| {
        eprintln!("error: no crate given");
        process::exit(2);
    });
    let me = env::current_exe().expect("current exe");
    let version_file = me.with_file_name("fixture-version");
    let version = fs::read_to_string(&version_file).map(|s| s.trim().to_string()).unwrap_or_else(|_| "0.1.0".into());

    eprintln!("    Updating crates.io index");
    eprintln!("  Downloaded {krate} v{version}");
    eprintln!("   Compiling {krate} v{version}");
    if krate == "fixture-slow" {
        thread::sleep(Duration::from_secs(8));
    }
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    if krate == "fixture-fail" || version == "fail" {
        fs::write(bin_dir.join("partial.o"), b"partial").ok();
        eprintln!("error: could not compile `{krate}` (bin \"{krate}\")");
        eprintln!("error: failed to compile `{krate} v{version}`");
        process::exit(101);
    }
    let names: Vec<String> = if bins.is_empty() { vec![krate.clone()] } else { bins.clone() };
    for name in &names {
        let target = bin_dir.join(format!("{name}{}", exe_suffix()));
        fs::copy(&me, &target).expect("copy fixture binary");
    }
    let key = format!("{krate} {version} (registry+https://github.com/rust-lang/crates.io-index)");
    let json = format!(
        "{{\"installs\":{{\"{key}\":{{\"version_req\":null,\"bins\":[{}],\"features\":[],\"all_features\":false,\"no_default_features\":false,\"profile\":\"release\",\"target\":\"fixture\",\"rustc\":\"fixture\"}}}}}}",
        names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(",")
    );
    fs::write(root.join(".crates2.json"), json).expect("write crates2");
    for name in &names {
        eprintln!("  Installing {}", bin_dir.join(name).display());
    }
    eprintln!("   Installed package `{krate} v{version}` (executable{} {})", if names.len() > 1 { "s" } else { "" }, names.join(", "));
}

fn fake_game(args: &[String]) {
    let mut exit_code = 0;
    let mut i = 0;
    println!("fixture game running");
    while i < args.len() {
        match args[i].as_str() {
            "--exit" => {
                exit_code = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
                i += 2;
            }
            "--sleep-ms" => {
                let ms = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
                thread::sleep(Duration::from_millis(ms));
                i += 2;
            }
            "--print" => {
                println!("{}", args.get(i + 1).cloned().unwrap_or_default());
                i += 2;
            }
            "--pwd" => {
                println!("{}", env::current_dir().map(|p| p.display().to_string()).unwrap_or_default());
                i += 1;
            }
            "--echo-env" => {
                let name = args.get(i + 1).cloned().unwrap_or_default();
                println!("{}={}", name, env::var(&name).unwrap_or_default());
                i += 2;
            }
            "--crash" => process::abort(),
            _ => i += 1,
        }
    }
    process::exit(exit_code);
}
