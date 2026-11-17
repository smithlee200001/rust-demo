//! 文件日志的端到端微基准：使用同一批合成记录比较六种配置。
//!
//! 初始化包含加密流及口令 KDF，写入包含脱敏和缓冲写入，完成包含末块及 flush。
//! 不执行 fsync，因此结果衡量交给操作系统的成本，而非断电持久化延迟。
use std::{env, error::Error, io, path::Path, time::Instant};

use age::{secrecy::SecretString, x25519};
use log_demo::file_log::{FileLog, Mode};

/// 加密类别与脱敏开关分开建模，避免用显示名称驱动业务分支。
#[derive(Clone, Copy)]
enum Encryption {
    Plain,
    Symmetric,
    Asymmetric,
}

#[derive(Clone, Copy)]
struct Scenario {
    encryption: Encryption,
    redact: bool,
    label: &'static str,
}

const SCENARIOS: [Scenario; 6] = [
    Scenario {
        encryption: Encryption::Plain,
        redact: false,
        label: "plain",
    },
    Scenario {
        encryption: Encryption::Plain,
        redact: true,
        label: "redact",
    },
    Scenario {
        encryption: Encryption::Symmetric,
        redact: false,
        label: "symmetric",
    },
    Scenario {
        encryption: Encryption::Symmetric,
        redact: true,
        label: "symmetric+redact",
    },
    Scenario {
        encryption: Encryption::Asymmetric,
        redact: false,
        label: "asymmetric",
    },
    Scenario {
        encryption: Encryption::Asymmetric,
        redact: true,
        label: "asymmetric+redact",
    },
];

/// 每一轮保留独立的完整耗时；总耗时中位数不由各阶段中位数相加得到。
struct Sample {
    init: f64,
    write: f64,
    finish: f64,
    total: f64,
    bytes: u64,
}

struct Config {
    records: usize,
    rounds: usize,
}

/// 只接受有界的正整数，防止误输入使基准占用无界内存或运行极长时间。
/// 返回 None 表示仅显示帮助，不生成密钥、不创建文件。
fn config(args: impl IntoIterator<Item = String>) -> io::Result<Option<Config>> {
    let mut config = Config {
        records: 10_000,
        rounds: 5,
    };
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            println!("Usage: log-file-bench [--records 1..1000000] [--rounds 1..100]");
            return Ok(None);
        }
        let (target, max) = match flag.as_str() {
            "--records" => (&mut config.records, 1_000_000),
            "--rounds" => (&mut config.rounds, 100),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Unknown argument: {flag}"),
                ));
            }
        };
        let value = args
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| (1..=max).contains(value))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{flag} requires an integer in 1..={max}"),
                )
            })?;
        *target = value;
    }
    Ok(Some(config))
}

/// 密钥生成、测试文本生成和 Mode 克隆均在计时之外；create 内的密码学初始化计入 init。
/// 文件路径必须位于本次独占的临时目录，避免覆盖用户已有日志。
fn measure(
    path: &Path,
    scenario: Scenario,
    lines: &[String],
    recipient: &x25519::Recipient,
) -> io::Result<Sample> {
    let mode = match scenario.encryption {
        Encryption::Plain => Mode::Plain,
        Encryption::Symmetric => Mode::Symmetric(SecretString::from(
            "synthetic-benchmark-passphrase-not-for-real-data".to_owned(),
        )),
        Encryption::Asymmetric => Mode::Asymmetric(recipient.clone()),
    };
    let start = Instant::now();
    let mut log = FileLog::create(path, mode, scenario.redact)?;
    let initialized = Instant::now();
    for line in lines {
        log.append(line)?;
    }
    let written = Instant::now();
    log.finish()?;
    let finished = Instant::now();
    Ok(Sample {
        init: initialized.duration_since(start).as_secs_f64(),
        write: written.duration_since(initialized).as_secs_f64(),
        finish: finished.duration_since(written).as_secs_f64(),
        total: finished.duration_since(start).as_secs_f64(),
        bytes: std::fs::metadata(path)?.len(),
    })
}

/// 只用于非空有限样本；偶数个样本取中间两数平均，不偏向某一轮。
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// 轮次间旋转配置顺序，降低固定顺序与缓存/温度变化的系统性偏差。
/// 每轮完成后删除自己的文件，错误退出由 TempDir 回收；不读取或删除任何真实日志。
fn main() -> Result<(), Box<dyn Error>> {
    let Some(config) = config(env::args().skip(1))? else {
        return Ok(());
    };
    let temp = tempfile::Builder::new()
        .prefix("log-file-bench-")
        .tempdir()?;
    let recipient = x25519::Identity::generate().to_public();
    let lines: Vec<_> = (0..config.records).map(|index| format!(
        "INFO record={index:08} unix=/home/alice/work/report.txt mac=/Users/bob/Documents/data.csv windows=C:\\Users\\carol\\AppData\\Local\\demo.log status=ok"
    )).collect();
    let input_bytes: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut samples: [Vec<Sample>; 6] = std::array::from_fn(|_| Vec::with_capacity(config.rounds));
    println!(
        "File logging benchmark: {} / {}",
        env::consts::OS,
        env::consts::ARCH
    );
    println!(
        "Records: {}; rounds: {}; input bytes per file: {input_bytes}",
        config.records, config.rounds
    );
    println!(
        "Synthetic inputs only; setup/key generation excluded except FileLog::create. No warm-up trials."
    );
    println!(
        "Buffered writes + finish/flush, no fsync. Results do not measure power-loss durability."
    );
    println!("Symmetric init includes age scrypt; asymmetric uses age X25519 hybrid encryption.");
    if cfg!(debug_assertions) {
        println!(
            "WARNING: Debug build. Use cargo run --release --bin log-file-bench for meaningful comparisons."
        );
    }
    for round in 0..config.rounds {
        eprintln!("Running round {}/{}...", round + 1, config.rounds);
        for offset in 0..SCENARIOS.len() {
            let index = (round + offset) % SCENARIOS.len();
            let path = temp
                .path()
                .join(format!("round-{round}-scenario-{index}.log"));
            let sample = measure(&path, SCENARIOS[index], &lines, &recipient)?;
            samples[index].push(sample);
            std::fs::remove_file(path)?;
        }
    }
    let baseline = median(samples[0].iter().map(|sample| sample.total).collect());
    println!(
        "\nMedians (ms); total ratio is relative to plain; write us/record excludes init and finish:"
    );
    println!(
        "{:<20} {:>11} {:>11} {:>11} {:>11} {:>9} {:>13} {:>13}",
        "scenario",
        "init_ms",
        "write_ms",
        "finish_ms",
        "total_ms",
        "total_x",
        "write_us/rec",
        "file_bytes"
    );
    for (scenario, results) in SCENARIOS.iter().zip(&samples) {
        let init = median(results.iter().map(|sample| sample.init).collect());
        let write = median(results.iter().map(|sample| sample.write).collect());
        let finish = median(results.iter().map(|sample| sample.finish).collect());
        let total = median(results.iter().map(|sample| sample.total).collect());
        let bytes = median(results.iter().map(|sample| sample.bytes as f64).collect());
        println!(
            "{:<20} {:>11.3} {:>11.3} {:>11.3} {:>11.3} {:>9.2} {:>13.3} {:>13.0}",
            scenario.label,
            init * 1e3,
            write * 1e3,
            finish * 1e3,
            total * 1e3,
            total / baseline,
            write * 1e6 / config.records as f64,
            bytes
        );
    }
    println!(
        "\nCompare redacted and unredacted rows within each encryption mode to isolate redaction overhead."
    );
    println!(
        "Short files amplify one-time scrypt costs. Repeat on your target storage and release build."
    );
    temp.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_rejects_zero_missing_unknown_and_excessive_values() {
        for args in [
            vec!["--records", "0"],
            vec!["--rounds"],
            vec!["--wrong", "1"],
            vec!["--records", "1000001"],
            vec!["--rounds", "101"],
        ] {
            assert!(config(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn cli_parses_both_limits() {
        let config = config(
            ["--records", "25", "--rounds", "2"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("Valid CLI arguments")
        .expect("Benchmark config");
        assert_eq!(config.records, 25);
        assert_eq!(config.rounds, 2);
    }

    #[test]
    fn median_supports_odd_and_even_round_counts() {
        assert_eq!(median(vec![5.0, 1.0, 3.0]), 3.0);
        assert_eq!(median(vec![8.0, 2.0, 4.0, 6.0]), 5.0);
        assert_eq!(median(vec![2.0]), 2.0);
    }
}
