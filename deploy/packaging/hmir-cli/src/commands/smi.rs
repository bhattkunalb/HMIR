use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Table, Cell, Color as TableColor, Attribute};
use colored::Colorize;
use serde_json::Value;
use std::time::Duration;

pub async fn run_smi(port: u16) {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/v1/hardware/snapshot", port);

    println!("{}", "HMIR ELITE | System Management Interface".bold().cyan());
    println!("{}\n", "-----------------------------------------".cyan());

    match client.get(&url).timeout(Duration::from_secs(2)).send().await {
        Ok(resp) => {
            if let Ok(snapshot) = resp.json::<Value>().await {
                render_table(&snapshot);
            } else {
                println!("{} Failed to parse telemetry response.", "❌".red());
            }
        }
        Err(_) => {
            println!("{} HMIR Node is offline.", "❌".red());
            println!("💡 Start it with: {} {}", "hmir".bold(), "start");
        }
    }
}

fn render_table(data: &Value) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            Cell::new("Compute Unit").add_attribute(Attribute::Bold).fg(TableColor::Cyan),
            Cell::new("Utilization").add_attribute(Attribute::Bold).fg(TableColor::Cyan),
            Cell::new("Temp").add_attribute(Attribute::Bold).fg(TableColor::Cyan),
            Cell::new("Memory / VRAM").add_attribute(Attribute::Bold).fg(TableColor::Cyan),
            Cell::new("Driver / Specs").add_attribute(Attribute::Bold).fg(TableColor::Cyan),
        ]);

    // CPU Row
    table.add_row(vec![
        Cell::new(format!("󰻠 CPU: {}", data["cpu_name"].as_str().unwrap_or("Unknown"))).fg(TableColor::White),
        Cell::new(format!("{:.1}%", data["cpu_util"].as_f64().unwrap_or(0.0))).fg(TableColor::Green),
        Cell::new(format!("{:.0}°C", data["cpu_temp"].as_f64().unwrap_or(0.0))).fg(TableColor::Yellow),
        Cell::new(format!("{:.1}/{:.1} GB", 
            data["ram_used"].as_f64().unwrap_or(0.0),
            data["ram_total"].as_f64().unwrap_or(0.0)
        )),
        Cell::new(format!("{} Cores / {} MT", 
            data["cpu_cores"].as_u64().unwrap_or(0),
            data["cpu_threads"].as_u64().unwrap_or(0)
        )),
    ]);

    // GPU Row
    table.add_row(vec![
        Cell::new(format!("󰢮 GPU: {}", data["gpu_name"].as_str().unwrap_or("None"))).fg(TableColor::Cyan),
        Cell::new(format!("{:.1}%", data["gpu_util"].as_f64().unwrap_or(0.0))).fg(TableColor::Green),
        Cell::new(format!("{:.0}°C", data["gpu_temp"].as_f64().unwrap_or(0.0))).fg(TableColor::Yellow),
        Cell::new(format!("{:.1}/{:.1} GB", 
            data["vram_used"].as_f64().unwrap_or(0.0),
            data["vram_total"].as_f64().unwrap_or(0.0)
        )),
        Cell::new(data["gpu_driver"].as_str().unwrap_or("N/A").to_string()),
    ]);

    // NPU Row (Priority)
    table.add_row(vec![
        Cell::new(format!("󰚩 NPU: {}", data["npu_name"].as_str().unwrap_or("None"))).fg(TableColor::Magenta),
        Cell::new(format!("{:.1}%", data["npu_util"].as_f64().unwrap_or(0.0))).fg(TableColor::Green),
        Cell::new("--").fg(TableColor::DarkGrey),
        Cell::new(format!("{:.1} GB Shared", data["npu_vram_used"].as_f64().unwrap_or(0.0))),
        Cell::new(data["npu_driver"].as_str().unwrap_or("Intel AI Boost").to_string()),
    ]);

    println!("{table}");

    // Additional Stats
    println!("\n{}", "System Context:".bold().white());
    println!("  󰋊 Disk:    {:.1}/{:.1} GB Free ({})", 
        data["disk_free"].as_f64().unwrap_or(0.0),
        data["disk_total"].as_f64().unwrap_or(0.0),
        data["disk_model"].as_str().unwrap_or("N/A")
    );
    println!("  󰏖 Uptime:  {}s", data["node_uptime"].as_u64().unwrap_or(0));
    println!("  󱐋 Power:   {:.1} W", data["power_w"].as_f64().unwrap_or(0.0));
    println!("  󰓅 Engine:  {}", data["engine_status"].as_str().unwrap_or("Idle").bold().green());
}
