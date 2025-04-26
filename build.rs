use std::{env, fs};
use std::path::PathBuf;
use bindgen::{EnumVariation, NonCopyUnionStyle, RustTarget};

fn main() {
    println!("cargo:rustc-link-search=/external/HDiffPatch");
    /*println!("cargo:rustc-link-lib=hdiffz");
    println!("cargo:rustc-link-lib=hpatchz");*/
    println!("cargo:rustc-link-lib=hdiffpatch");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .layout_tests(false)
        .generate_inline_functions(true)
        .default_enum_style(EnumVariation::Rust { non_exhaustive: false })
        .manually_drop_union(".*")
        .default_non_copy_union_style(NonCopyUnionStyle::ManuallyDrop)
        .rust_target(RustTarget::nightly())
        //.allowlist_type(".*")
        .clang_arg("-std=c++14")
        .clang_arg("-xc++")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    
    // Hacky replace for bindgen bug: https://github.com/rust-lang/rust-bindgen/pull/2764
    let thing = bindings.to_string().replace("_M_val: _Tp", "_M_val: std::mem::ManuallyDrop<_Tp>");
    // Replace double fields and other bullshit lmfao fuck you
    let thing = thing.replace("pub _base: hpatch_TStreamOutput,", "").replacen("pub type iterator = std__Bit_iterator;", "", 1)
        .replacen("pub type size_type = usize;", "", 1).replacen("pub type size_type = size_type;", "", 1)
        .replacen("pub static std_value: _Tp;", "pub static std_value: u8;", 1).replacen("pub static __gnu_cxx___min: _Value;", "pub static __gnu_cxx___min: u8;", 1)
        .replacen("pub static __gnu_cxx___max: _Value;", "pub static __gnu_cxx___max: u8;", 1);
    fs::write(out_path.join("bindings.rs"), thing).expect("Unable to write bindings");
}