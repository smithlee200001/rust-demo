//! 构建脚本：在编译期使用内置的 `protoc` 将 `.proto` 文件编译为 Rust 结构体。

fn main() {
    println!("cargo:rerun-if-changed=proto/ipc_messages.proto");

    // 获取由 protoc-bin-vendored 提供的免安装 protoc 可执行文件路径
    let protoc_path =
        protoc_bin_vendored::protoc_bin_path().expect("Failed to locate vendored protoc binary");

    // Rust 2024 Edition 中修改环境变量属于 unsafe 操作（单线程构建脚本中安全）
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    let mut config = prost_build::Config::new();
    // 为方案三提供支持：给所有由 .proto 生成的 Rust 结构体自动加上 serde 的 Serialize / Deserialize 派生宏
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");

    config
        .compile_protos(&["proto/ipc_messages.proto"], &["proto/"])
        .expect("Failed to compile protobuf definitions");
}
