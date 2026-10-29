//! Embeds Horizon's icon (`packaging/icons/horizon.ico`) in the Windows executables, as icon
//! resource 1: Explorer and the task bar show it, and the window takes it as its icon.
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    env_file(&format!("{dir}/../.env"));
    let ico = format!("{dir}/../packaging/icons/horizon.ico");
    println!("cargo:rerun-if-changed={ico}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(&ico);
        if let Err(e) = resource.compile() {
            panic!("embedding {ico}: {e}");
        }
    }
}

/// The build-time keys (`option_env!`) a local `.env` at the repository's root may give
/// (`KEY=value` lines; `#` comments). It is gitignored, never committed; a variable set in the
/// shell wins over it, and CI sets them there.
const ENV_KEYS: [&str; 6] = [
    "HORIZON_MSA_CLIENT_ID",
    "HORIZON_CURSEFORGE_API_KEY",
    "HORIZON_GITHUB_CLIENT_ID",
    "HORIZON_GOOGLE_CLIENT_ID",
    "HORIZON_GOOGLE_CLIENT_SECRET",
    "HORIZON_LAUNCHER_NO_SELF_UPDATE",
];

fn env_file(path: &str) {
    println!("cargo:rerun-if-changed={path}");
    for key in ENV_KEYS {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if line.starts_with('#') || !ENV_KEYS.contains(&key) || std::env::var_os(key).is_some() {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        println!("cargo:rustc-env={key}={value}");
    }
}
