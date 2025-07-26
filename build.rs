use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

fn main() {
    embed_resource::compile("assets/logo.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("images.rs");

    // 获取项目根目录
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir);
    let icons_dir = manifest_path.join("assets/battery_icons"); // 使用绝对路径

    let mut file = BufWriter::new(File::create(dest_path).unwrap());

    let mut map_builder = phf_codegen::Map::new();
    for entry in fs::read_dir(&icons_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("png") {
            // 使用文件名（不含扩展名）作为 key
            let key = path.file_stem().unwrap().to_str().unwrap().to_string();

            let value_path = path.to_str().unwrap();

            // 将 "文件名" -> include_bytes!("文件路径") 添加到 map 中
            // 注意：需要转义 Windows 路径中的反斜杠
            let include_bytes = format!("include_bytes!(\"{}\")", value_path.replace('\\', "/"));
            map_builder.entry(key, include_bytes);
        }
    }

    write!(
        &mut file,
        "static IMAGES: phf::Map<&'static str, &'static [u8]> = {}",
        map_builder.build()
    )
    .unwrap();

    writeln!(&mut file, ";\n").unwrap();

    // 通知 Cargo：当 build.rs 或 assets/battery_icons 目录内容变化时，重新运行此脚本
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/battery_icons");
}
