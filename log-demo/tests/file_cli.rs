//! 用真实命令行和子进程验证文件模式，不借用用户文件或真实密钥。
use std::{fs, io::Read, process::Command};

#[test]
fn recipient_mode_captures_structured_and_child_output_once()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("roundtrip.age");
    let identity = age::x25519::Identity::generate();
    let output = Command::new(env!("CARGO_BIN_EXE_log-demo"))
        .arg("--file")
        .arg(&path)
        .args([
            "--encryption",
            "asymmetric",
            "--recipient",
            &identity.to_public().to_string(),
        ])
        .output()?;
    assert!(output.status.success(), "{:?}", output.stderr);
    // 文件模式不能把明文再复制到进程的输出流。
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let cipher = fs::read(path)?;
    let mut clear = String::new();
    age::Decryptor::new(&cipher[..])?
        .decrypt(std::iter::once(&identity as &dyn age::Identity))?
        .read_to_string(&mut clear)?;
    for expected in [
        "path=/home/***/report.txt",
        "Worker initialized: loading configuration at /Users/***/config",
        r"C:\\Users\\***\\config",
        "All log demonstrations completed successfully",
    ] {
        assert_eq!(
            clear.matches(expected).count(),
            1,
            "Expected one record: {expected}"
        );
    }
    for raw in ["demo_linux_user", "demo_child_user", "demo_child_windows"] {
        assert!(!clear.contains(raw), "Username leaked: {raw}");
    }
    Ok(())
}

#[test]
fn invalid_encryption_configuration_does_not_create_or_print_a_log() -> std::io::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("not-written.age");
    let output = Command::new(env!("CARGO_BIN_EXE_log-demo"))
        .arg("--file")
        .arg(&path)
        .args(["--encryption", "asymmetric"])
        .output()?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!path.exists());
    Ok(())
}
