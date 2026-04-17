use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use hmir_core::telemetry::{TelemetrySink, TelemetryEvent};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HardwareState {
    pub cpu_name: String,
    pub cpu_util_pct: f64,
    pub gpu_name: String,
    pub gpu_util_pct: f64,
    pub npu_name: String,
    pub npu_util_pct: f64,
    pub vram_used_bytes: u64,
    pub vram_total_bytes: u64,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub power_draw_watts: f64,
    pub cpu_temp_c: f64,
    pub gpu_temp_c: f64,
    pub disk_free_gb: f64,
}

#[cfg(target_os = "windows")]
pub mod os_polling {
    use super::*;
    use sysinfo::{System, Components};

    pub async fn poll_hardware() -> HardwareState {
        let mut sys = System::new_all();
        
        // Wait a small amount for CPU stats to populate
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        sys.refresh_cpu();
        sys.refresh_memory();

        let components = Components::new_with_refreshed_list();
        let mut cpu_temp = components.iter()
            .find(|c| c.label().to_lowercase().contains("cpu") || c.label().to_lowercase().contains("core"))
            .map(|c| c.temperature() as f64)
            .unwrap_or(0.0);
        
        let mut gpu_temp = components.iter()
            .find(|c| c.label().to_lowercase().contains("gpu") || c.label().to_lowercase().contains("graphics"))
            .map(|c| c.temperature() as f64)
            .unwrap_or(0.0);

        // Fallback for restricted Windows systems
        if cpu_temp == 0.0 {
            if let Ok(output) = std::process::Command::new("powershell")
                .args(["-Command", "Get-CimInstance Win32_PerfRawData_Counters_ThermalZoneInformation | Select-Object -ExpandProperty HighPrecisionTemperature -First 1"])
                .output()
            {
                let raw = String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().unwrap_or(0.0);
                if raw > 0.0 {
                    // Smart Scaling Logic:
                    // 1. If > 2700, it's likely Tenths of Kelvin (e.g. 3000 = 26.8C)
                    // 2. If < 1000 (and > 100), it's likely Decicelsius (e.g. 350 = 35C)
                    // 3. Otherwise treat as Celsius or raw error
                    if raw > 2700.0 {
                        cpu_temp = (raw * 0.1) - 273.15;
                    } else if raw > 100.0 {
                        cpu_temp = raw * 0.1;
                    } else {
                        cpu_temp = raw;
                    }
                    
                    // For integrated systems, CPU and GPU often share thermal zones
                    if gpu_temp == 0.0 {
                        gpu_temp = cpu_temp;
                    }
                }
            }
        }

        let cpu_name = sys.cpus().first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());
        
        let cpu_util = sys.global_cpu_info().cpu_usage() as f64;
        // Task Manager RAM: sys.used_memory() is more like "Committed", but users prefer "In Use"
        // Let's provide Total and Used in bits that make sense (bytes -> GB)
        let ram_used = sys.used_memory(); 
        let ram_total = sys.total_memory();

        let (gpu_name, npu_name) = probe_accelerators();

        // High-Precision NPU Polling for Intel AI Boost
        let npu_util = if npu_name != "None" {
            // Target 'Compute' since Intel AI Boost exposes utilization there under GPUEngine
            if let Ok(output) = std::process::Command::new("powershell")
                .args(["-Command", "Get-CimInstance -Query \"SELECT UtilizationPercentage FROM Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine WHERE Name LIKE '%Compute%'\" | Measure-Object -Property UtilizationPercentage -Maximum | Select-Object -ExpandProperty Maximum"])
                .output()
            {
                let raw = String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().unwrap_or(0.0);
                raw.min(100.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        let mut disk_free = 0.0;
        let disks = sysinfo::Disks::new_with_refreshed_list();
        if let Some(disk) = disks.iter().next() {
            disk_free = disk.available_space() as f64 / (1024.0 * 1024.0 * 1024.0);
        }

        HardwareState {
            cpu_name,
            cpu_util_pct: cpu_util,
            gpu_name,
            gpu_util_pct: 0.0,
            npu_name,
            npu_util_pct: npu_util,
            vram_used_bytes: 0,
            vram_total_bytes: 0,
            ram_used_bytes: ram_used,
            ram_total_bytes: ram_total,
            power_draw_watts: 0.0,
            cpu_temp_c: cpu_temp,
            gpu_temp_c: gpu_temp,
            disk_free_gb: disk_free,
        }
    }

    fn probe_accelerators() -> (String, String) {
        let mut gpu = "Integrated Graphics".to_string();
        let mut npu = "None".to_string();

        // Check for ComputeAccelerator class (Intel AI Boost, Qualcomm)
        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-Command", "Get-PnpDevice -Class 'ComputeAccelerator' -ErrorAction SilentlyContinue | Where-Object { $_.Status -eq 'OK' } | Select-Object -ExpandProperty FriendlyName"])
            .output() 
        {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                npu = name;
            }
        }

        // Fallback: Check PnPEntity for "NPU" or "Neural" strings
        if npu == "None" {
            if let Ok(output) = std::process::Command::new("powershell")
                .args(["-Command", "Get-CimInstance Win32_PnPEntity | Where-Object { $_.Name -match 'NPU|Neural|AI Boost|Hexagon' } | Select-Object -ExpandProperty Name -First 1"])
                .output()
            {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !name.is_empty() {
                    npu = name;
                }
            }
        }

        // Check for GPU
        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-Command", "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name"])
            .output() 
        {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                gpu = name;
            }
        }

        (gpu, npu)
    }
}

#[cfg(target_os = "linux")]
pub mod os_polling {
    use super::*;
    use sysinfo::{System, SystemExt, CpuExt};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    static LAST_NPU_BUSY_TIME: AtomicU64 = AtomicU64::new(0);
    static LAST_NPU_POLL_TIME: OnceLock<Instant> = OnceLock::new();
    use std::sync::OnceLock;

    pub async fn poll_hardware() -> HardwareState {
        let mut sys = System::new_all();
        sys.refresh_cpu();
        sys.refresh_memory();

        let cpu_name = sys.cpus().first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());
        
        let cpu_util = sys.global_cpu_info().cpu_usage() as f64;
        let ram_used = sys.used_memory();
        let ram_total = sys.total_memory();

        // NPU Probing (Intel ivpu driver)
        let mut npu_name = "None".to_string();
        let mut npu_util = 0.0;
        
        if fs::metadata("/sys/class/accel/accel0").is_ok() {
            npu_name = "Intel NPU (ivpu)".to_string();
            
            // Calculate utilization if busy_time_us is available
            if let Ok(busy_str) = fs::read_to_string("/sys/class/accel/accel0/device/npu_busy_time_us") {
                if let Ok(busy_us) = busy_str.trim().parse::<u64>() {
                    let now = Instant::now();
                    let last_time = LAST_NPU_POLL_TIME.get_or_init(|| now);
                    let last_busy = LAST_NPU_BUSY_TIME.swap(busy_us, Ordering::SeqCst);
                    
                    let elapsed = now.duration_since(*last_time).as_micros() as u64;
                    if elapsed > 0 && last_busy > 0 {
                        let delta_busy = busy_us.saturating_sub(last_busy);
                        npu_util = (delta_busy as f64 / elapsed as f64) * 100.0;
                    }
                    
                    // Note: This static approach is a bit hacky for a shared library,
                    // but effective for the background loop singleton.
                    // We'd ideally pass state if we had multiple probers.
                }
            }
        }

        HardwareState {
            cpu_name,
            cpu_util_pct: cpu_util,
            gpu_name: "Generic GPU (Linux)".to_string(), // Placeholder, needs specific driver libs
            gpu_util_pct: 0.0,
            npu_name,
            npu_util_pct: npu_util.min(100.0),
            vram_used_bytes: 0,
            ram_used_bytes: ram_used,
            ram_total_bytes: ram_total,
            power_draw_watts: 0.0,
            cpu_temp_c: 0.0, // sysinfo::Components works on Linux too, but omitted for brevity
            gpu_temp_c: 0.0,
            disk_free_gb: sys.disks().first().map(|d| d.available_space() as f64 / 1_073_741_824.0).unwrap_or(0.0),
        }
    }
}

#[cfg(target_os = "macos")]
pub mod os_polling {
    use super::*;
    use sysinfo::{System, SystemExt, CpuExt};

    pub async fn poll_hardware() -> HardwareState {
        let mut sys = System::new_all();
        sys.refresh_cpu();
        sys.refresh_memory();

        let cpu_name = sys.cpus().first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Apple Silicon".to_string());
        
        let cpu_util = sys.global_cpu_info().cpu_usage() as f64;
        let ram_used = sys.used_memory();
        let ram_total = sys.total_memory();

        // ANE Detection (Apple Neural Engine) via sysctl
        let mut npu_name = "None".to_string();
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "hw.optional.ane"])
            .output() 
        {
            let val = String::from_utf8_lossy(&output.stdout).trim();
            if val == "1" {
                npu_name = "Apple Neural Engine (ANE)".to_string();
            }
        }

        HardwareState {
            cpu_name,
            cpu_util_pct: cpu_util,
            gpu_name: "Apple M-Series GPU (Metal)".to_string(),
            gpu_util_pct: 0.0,
            npu_name,
            npu_util_pct: 0.0, // ANE utilization metrics are not natively exposed in sysctl
            vram_used_bytes: 0,
            ram_used_bytes: ram_used,
            ram_total_bytes: ram_total,
            power_draw_watts: 0.0,
            cpu_temp_c: 0.0,
            gpu_temp_c: 0.0,
            disk_free_gb: sys.disks().first().map(|d| d.available_space() as f64 / 1_073_741_824.0).unwrap_or(0.0),
        }
    }
}

pub struct HardwareProber {
    pub sample_interval: Duration,
    pub state: Arc<RwLock<HardwareState>>,
}

impl HardwareProber {
    pub fn spawn_background_loop(self, sink: TelemetrySink) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.sample_interval);
            loop {
                interval.tick().await; 
                let hw = os_polling::poll_hardware().await;
                
                let mut guard = self.state.write().await;
                *guard = hw.clone();
                
                let _ = sink.emit(TelemetryEvent::HardwareState {
                    cpu_util: hw.cpu_util_pct,
                    gpu_util: hw.gpu_util_pct,
                    npu_util: hw.npu_util_pct,
                    cpu_temp: hw.cpu_temp_c,
                    gpu_temp: hw.gpu_temp_c,
                    vram_used: hw.vram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                    vram_total: hw.vram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                    ram_used: hw.ram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                    ram_total: hw.ram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                    tps: 0.0,
                    power_w: hw.power_draw_watts,
                    node_uptime: 0,
                    kv_cache: 0.0,
                    cpu_name: hw.cpu_name.clone(),
                    gpu_name: hw.gpu_name.clone(),
                    npu_name: hw.npu_name.clone(),
                    disk_free: hw.disk_free_gb,
                });
            }
        });
    }
}
