use std::env;
use std::fs;
use std::path::PathBuf;

use rslisp_expand::expand::PreludeState;

fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let state = output.join("prelude-state.bin");
    fs::write(&state, PreludeState::build().save()).expect("OUT_DIR takes the prelude state");
    println!("cargo::rustc-env=PRELUDE_STATE={}", state.display());
    println!("cargo::rerun-if-changed=build.rs");
}
