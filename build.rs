use std::fs;

fn main() {
    // 指定安装目录
    // 获取用户的主目录
    let home_dir = std::env::var("HOME").expect("Could not find home directory");
    let cargo_dir = std::path::Path::new(&home_dir).join(".cargo");
    let dest_path = cargo_dir.join("rust-toolchain.toml");

    fs::create_dir_all(&cargo_dir).unwrap();
    fs::copy("./rust-toolchain.toml", &dest_path).expect("Failed to copy to system directory");
}
