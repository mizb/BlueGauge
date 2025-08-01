use glob::glob;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

fn main() {
    load_logo();
    build_battery_icons_bytes();

    // 通知 Cargo：当 build.rs 或 assets/battery_icons 目录内容变化时，重新运行此脚本
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/battery_icons");
}

fn load_logo() {
    embed_resource::compile("assets/logo.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}

fn build_battery_icons_bytes() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("images.rs");

    // 获取项目根目录
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let icons_dir = Path::new(&manifest_dir).join("assets/battery_icons");

    assert!(
        icons_dir.exists(),
        "Icons directory does not exist: {icons_dir:?}"
    );

    let mut file = BufWriter::new(File::create(dest_path).unwrap());
    let mut map_builder = phf_codegen::Map::new();

    let pattern = format!("{}/*.png", icons_dir.to_string_lossy());
    for entry in glob(&pattern).expect("Failed to read glob pattern") {
        let path = entry.expect("Failed to get path");

        if path.is_file() {
            let key = path.file_stem().unwrap().to_str().unwrap().to_string();
            let value_path = path.to_str().unwrap().replace('\\', "/");

            map_builder.entry(key, format!("include_bytes!(\"{value_path}\")"));
        }
    }

    writeln!(
        &mut file,
        "static IMAGES: phf::Map<&'static str, &'static [u8]> = {};",
        map_builder.build()
    )
    .unwrap();
}
