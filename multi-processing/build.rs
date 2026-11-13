//! 构建脚本：在编译期使用内置的 `protoc` 将 `.proto` 文件编译为 Rust 结构体。

/// 使用显式的 protoc 路径生成协议类型；构建失败时由 Cargo 展示错误并中止构建。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/ipc_messages.proto");

    // 获取由 protoc-bin-vendored 提供的免安装 protoc 可执行文件路径
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;

    let mut config = prost_build::Config::new();
    // 直接配置编译器路径，避免修改进程环境变量及引入不必要的 unsafe。
    config.protoc_executable(protoc_path);
    // 为方案三提供支持：给所有由 .proto 生成的 Rust 结构体自动加上 serde 的 Serialize / Deserialize 派生宏
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");

    config.compile_protos(&["proto/ipc_messages.proto"], &["proto/"])?;
    Ok(())
}
