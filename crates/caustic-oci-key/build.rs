use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

use build_rs::output;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by Cargo");
    let key_path = env::var("CAUSTIC_COSIGN_PUB")
        .ok()
        .filter(|s| !s.is_empty());

    let key_bytes = key_path.as_ref().map_or_else(Vec::new, |path| {
        output::rerun_if_changed(path);
        fs::read(Path::new(path))
            .unwrap_or_else(|e| panic!("failed to read CAUSTIC_COSIGN_PUB file {path}: {e}"))
    });

    let mut src = String::from("static COSIGN_KEY_BYTES: &[u8] = &[");
    for byte in &key_bytes {
        let _ = write!(src, "0x{byte:02X}, ");
    }
    src.push_str("];\n");

    let dest = Path::new(&out_dir).join("cosign_pub.rs");
    fs::write(&dest, src).unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));

    output::rerun_if_env_changed("CAUSTIC_COSIGN_PUB");
}
