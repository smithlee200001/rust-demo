//! 跨平台系统信息采集完整示例程序
//!
//! 本示例演示如何使用 `sysinfo` (0.35) 跨平台采集主机、CPU、内存、磁盘、网络、
//! 硬件温度传感器与进程等常见系统指标，无需调用 `sysctl`、`sw_vers`、`df` 等平台专用命令：
//! 1. 主机与操作系统信息（系统版本、内核版本、主机名、运行时长、系统负载）
//! 2. CPU 型号、核心数、频率与实时使用率（两次刷新计算差值）
//! 3. 物理内存与 Swap 空间使用情况
//! 4. 磁盘分区、文件系统、类型与可用空间
//! 5. 网络接口累计收发字节数、包数与 MAC 地址
//! 6. 硬件温度传感器读数（视宿主机权限与硬件支持而定）
//! 7. 当前进程与内存占用 Top 5 进程快照

use std::thread;
use sysinfo::{
    Components, CpuRefreshKind, Disks, MINIMUM_CPU_UPDATE_INTERVAL, MemoryRefreshKind, Networks,
    ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};

/// 跨平台核心系统信息摘要结构体（对应 `sysctl` / `sw_vers` 常见字段）
#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemInfo {
    /// CPU 品牌/型号名称（例如 `"Apple M3 Pro"` 或 `"Intel(R) Core(TM)..."`）
    chip: String,
    /// 总物理内存大小（单位：字节）
    memory_bytes: u64,
    /// 操作系统短版本号（例如 `"15.1"` 或 `"24.04"`）
    os_version: String,
    /// 操作系统名称（例如 `"Darwin"`、`"Ubuntu"`、`"Windows"`）
    os_name: String,
    /// 操作系统完整描述（例如 `"macOS 15.1 Sequoia"`）
    long_os_version: String,
    /// CPU 指令集架构（例如 `"aarch64"`、`"x86_64"`）
    arch: String,
    /// 逻辑 CPU 核心数
    logical_cores: usize,
    /// 物理 CPU 核心数（部分虚拟化或受限环境下可能返回 `None`）
    physical_cores: Option<usize>,
}

/// 采集当前宿主机的基础系统信息
///
/// 关键逻辑：
/// - 使用 `System::new_with_specifics` 仅开启 CPU 与物理内存刷新，避免扫描进程列表和 Swap，
///   从而将初始化开销降到最低。
/// - 边界条件：当平台或权限限制导致某些字符串字段为空或 `None` 时，统一回退为 `"unknown"`。
fn collect_system_info() -> SystemInfo {
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );

    let chip = sys
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let memory_bytes = sys.total_memory();
    let os_version = System::os_version().unwrap_or_else(|| "unknown".to_string());
    let os_name = System::name().unwrap_or_else(|| "unknown".to_string());
    let long_os_version = System::long_os_version().unwrap_or_else(|| "unknown".to_string());
    let arch = System::cpu_arch();
    let logical_cores = sys.cpus().len();
    let physical_cores = System::physical_core_count();

    SystemInfo {
        chip,
        memory_bytes,
        os_version,
        os_name,
        long_os_version,
        arch,
        logical_cores,
        physical_cores,
    }
}

/// 将字节数转换为 GiB（1 GiB = 1024^3 字节）
fn bytes_to_gib(bytes: u64) -> f64 {
    const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    bytes as f64 / BYTES_PER_GIB
}

/// 将字节数转换为 MiB（1 MiB = 1024^2 字节）
fn bytes_to_mib(bytes: u64) -> f64 {
    const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;
    bytes as f64 / BYTES_PER_MIB
}

/// 将运行秒数格式化为 `Xd Xh Xm Xs` 形式
///
/// 参数含义：
/// - `uptime_secs`: 系统自启动以来经过的秒数
fn format_uptime(uptime_secs: u64) -> String {
    let days = uptime_secs / 86_400;
    let hours = (uptime_secs % 86_400) / 3_600;
    let minutes = (uptime_secs % 3_600) / 60;
    let seconds = uptime_secs % 60;
    format!("{}d {}h {}m {}s", days, hours, minutes, seconds)
}

/// 1. 演示主机标识、操作系统版本、运行时间与系统负载
fn demo_host_and_os_info(info: &SystemInfo) {
    println!("=== 1. Host & OS Information ===");
    println!(
        "Host Name      : {}",
        System::host_name().unwrap_or_else(|| "unknown".to_string())
    );
    println!("OS Name        : {}", info.os_name);
    println!("OS Version     : {}", info.os_version);
    println!("OS Full        : {}", info.long_os_version);
    println!("Distribution   : {}", System::distribution_id());
    println!(
        "Kernel Version : {}",
        System::kernel_version().unwrap_or_else(|| "unknown".to_string())
    );
    let uptime = System::uptime();
    println!("Uptime         : {} ({}s)", format_uptime(uptime), uptime);
    println!("Boot Time      : {} (unix epoch)", System::boot_time());

    let load = System::load_average();
    println!(
        "Load Average   : 1m={:.2}, 5m={:.2}, 15m={:.2}",
        load.one, load.five, load.fifteen
    );
}

/// 2. 演示 CPU 型号、频率与实时使用率采集
///
/// 关键逻辑：
/// `sysinfo` 通过两次采样之间 CPU 时间片的差值计算使用率，因此至少需要间隔
/// `MINIMUM_CPU_UPDATE_INTERVAL` 后再次调用 `refresh_cpu_usage()` 才能得到准确读数。
fn demo_cpu_metrics(info: &SystemInfo) {
    println!("\n=== 2. CPU Information & Usage ===");
    let mut sys =
        System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()));
    thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();

    let vendor = sys
        .cpus()
        .first()
        .map(|cpu| cpu.vendor_id())
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown");
    let freq_mhz = sys.cpus().first().map(|cpu| cpu.frequency()).unwrap_or(0);

    println!("Chip / CPU     : {}", info.chip);
    println!("Vendor ID      : {}", vendor);
    println!("Architecture   : {}", info.arch);
    println!("Logical Cores  : {}", info.logical_cores);
    match info.physical_cores {
        Some(cores) => println!("Physical Cores : {}", cores),
        None => println!("Physical Cores : unknown"),
    }
    println!("Base Frequency : {} MHz", freq_mhz);
    println!("Global Usage   : {:.1}%", sys.global_cpu_usage());

    // 展示前 4 个逻辑核心的使用率，避免高核数机器刷屏
    for cpu in sys.cpus().iter().take(4) {
        println!(
            "  Core {:<6}  : {:.1}% ({} MHz)",
            cpu.name(),
            cpu.cpu_usage(),
            cpu.frequency()
        );
    }
    if sys.cpus().len() > 4 {
        println!("  ... ({} more cores omitted)", sys.cpus().len() - 4);
    }
}

/// 3. 演示物理内存与 Swap 空间详情
fn demo_memory_and_swap() {
    println!("\n=== 3. Memory & Swap ===");
    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
    );

    println!(
        "Total Memory     : {} bytes ({:.2} GiB)",
        sys.total_memory(),
        bytes_to_gib(sys.total_memory())
    );
    println!(
        "Used Memory      : {} bytes ({:.2} GiB)",
        sys.used_memory(),
        bytes_to_gib(sys.used_memory())
    );
    println!(
        "Available Memory : {} bytes ({:.2} GiB)",
        sys.available_memory(),
        bytes_to_gib(sys.available_memory())
    );
    println!(
        "Free Memory      : {} bytes ({:.2} GiB)",
        sys.free_memory(),
        bytes_to_gib(sys.free_memory())
    );
    println!(
        "Total Swap       : {} bytes ({:.2} GiB)",
        sys.total_swap(),
        bytes_to_gib(sys.total_swap())
    );
    println!(
        "Used Swap        : {} bytes ({:.2} GiB)",
        sys.used_swap(),
        bytes_to_gib(sys.used_swap())
    );
}

/// 4. 演示磁盘挂载点、文件系统与剩余空间
fn demo_disks() {
    println!("\n=== 4. Disks ===");
    let disks = Disks::new_with_refreshed_list();
    if disks.list().is_empty() {
        println!("No disks reported on this host");
        return;
    }

    for disk in &disks {
        println!(
            "Mount: {:<20} | FS: {:<8} | Kind: {:<7} | Removable: {:<5} | Avail: {:.2} / {:.2} GiB",
            disk.mount_point().display(),
            disk.file_system().to_string_lossy(),
            format!("{:?}", disk.kind()),
            disk.is_removable(),
            bytes_to_gib(disk.available_space()),
            bytes_to_gib(disk.total_space()),
        );
    }
}

/// 5. 演示网络接口流量统计与 MAC 地址
fn demo_networks() {
    println!("\n=== 5. Network Interfaces ===");
    let networks = Networks::new_with_refreshed_list();
    if networks.list().is_empty() {
        println!("No network interfaces reported on this host");
        return;
    }

    let mut entries: Vec<_> = networks.iter().collect();
    entries.sort_by_key(|(a, _)| *a);

    for (name, data) in entries {
        // 仅展示有流量或存在错误统计的活动接口，保持输出精简可读
        if data.total_received() == 0 && data.total_transmitted() == 0 {
            continue;
        }
        println!(
            "Interface: {:<10} | MAC: {} | RX: {:.2} MiB ({} pkts) | TX: {:.2} MiB ({} pkts)",
            name,
            data.mac_address(),
            bytes_to_mib(data.total_received()),
            data.total_packets_received(),
            bytes_to_mib(data.total_transmitted()),
            data.total_packets_transmitted(),
        );
    }
}

/// 6. 演示硬件温度传感器读数
fn demo_components() {
    println!("\n=== 6. Hardware Temperature Sensors ===");
    let components = Components::new_with_refreshed_list();
    if components.list().is_empty() {
        println!("No hardware temperature sensors reported on this host");
        return;
    }

    for component in components.iter().take(6) {
        let temp = component
            .temperature()
            .map(|t| format!("{:.1} C", t))
            .unwrap_or_else(|| "N/A".to_string());
        let max_temp = component
            .max()
            .map(|t| format!("{:.1} C", t))
            .unwrap_or_else(|| "N/A".to_string());
        println!(
            "Sensor: {:<28} | Current: {:<8} | Max: {}",
            component.label(),
            temp,
            max_temp
        );
    }
    if components.list().len() > 6 {
        println!("... ({} more sensors omitted)", components.list().len() - 6);
    }
}

/// 7. 演示当前进程信息与内存占用 Top 5 进程快照
fn demo_processes() {
    println!("\n=== 7. Processes Snapshot ===");
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );

    if let Ok(pid) = sysinfo::get_current_pid()
        && let Some(proc_info) = sys.process(pid)
    {
        println!(
            "Current Process: pid={}, name={}, rss={:.2} MiB, virt={:.2} MiB",
            pid,
            proc_info.name().to_string_lossy(),
            bytes_to_mib(proc_info.memory()),
            bytes_to_mib(proc_info.virtual_memory())
        );
    }

    let mut procs: Vec<_> = sys.processes().values().collect();
    procs.sort_by_key(|p| std::cmp::Reverse(p.memory()));

    println!("Top 5 Processes by Memory:");
    for proc_info in procs.into_iter().take(5) {
        println!(
            "  pid={:<7} | rss={:>8.2} MiB | name={}",
            proc_info.pid(),
            bytes_to_mib(proc_info.memory()),
            proc_info.name().to_string_lossy()
        );
    }
}

fn main() {
    println!("Collecting cross-platform system information...");

    let info = collect_system_info();
    demo_host_and_os_info(&info);
    demo_cpu_metrics(&info);
    demo_memory_and_swap();
    demo_disks();
    demo_networks();
    demo_components();
    demo_processes();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_system_info_returns_valid_host_metrics() {
        let info = collect_system_info();

        assert!(info.memory_bytes > 0, "total memory should be positive");
        assert!(
            info.logical_cores > 0,
            "logical core count should be positive"
        );
        assert!(!info.chip.is_empty(), "chip string should not be empty");
        assert!(
            !info.os_version.is_empty(),
            "os_version string should not be empty"
        );
        assert!(!info.arch.is_empty(), "arch string should not be empty");
    }

    #[test]
    fn bytes_conversion_helpers_convert_accurately() {
        assert_eq!(bytes_to_gib(0), 0.0);
        assert_eq!(bytes_to_mib(0), 0.0);

        let one_gib = 1024_u64 * 1024 * 1024;
        let one_mib = 1024_u64 * 1024;
        assert!((bytes_to_gib(one_gib) - 1.0).abs() < f64::EPSILON);
        assert!((bytes_to_mib(one_mib) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn format_uptime_formats_days_hours_minutes_seconds() {
        assert_eq!(format_uptime(0), "0d 0h 0m 0s");
        assert_eq!(format_uptime(90_061), "1d 1h 1m 1s");
    }
}
