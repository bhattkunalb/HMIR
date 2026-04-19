pub fn print_integrations(port: u16, model: Option<&str>) {
    let base_url = format!("http://127.0.0.1:{}/v1", port);
    let model = model.unwrap_or("qwen2.5-1.5b-ov");

    println!("HMIR OpenAI-Compatible Integrations");
    println!("-----------------------------------");
    println!("Base URL : {}", base_url);
    println!("API Key  : hmir-local");
    println!("Model    : {}", model);
    println!();
    println!("Generic OpenAI SDK");
    println!("  OPENAI_BASE_URL={}", base_url);
    println!("  OPENAI_API_KEY=hmir-local");
    println!("  model={}", model);
    println!();
    println!("Editor / app integrations");
    println!("  Cursor / editor plugins:");
    println!("    choose an OpenAI-compatible or custom OpenAI provider");
    println!("    set base URL to {}", base_url);
    println!("    set API key to hmir-local");
    println!("  VS Code AI extensions:");
    println!("    use the same base URL and API key");
    println!("  OpenClaw / OpenJarvis / Antigravity / Open WebUI:");
    println!("    point them at the same OpenAI-compatible endpoint when custom providers are supported");
    println!();
    println!("Shell example");
    println!(
        "  curl {}/chat/completions -H \"Content-Type: application/json\" -d '{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"hello\"}}]}}'",
        base_url, model
    );
    println!();
    println!("Tip: exact settings labels vary by client version, but the required inputs are always the same:");
    println!("  base URL, API key, and model name.");
}
