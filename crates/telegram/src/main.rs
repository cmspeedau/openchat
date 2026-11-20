use openchat_core::init_tracing;
use openchat_telegram::run;
use std::env;

fn main() {
    init_tracing();
    let token = match env::var("TELEGRAM_BOT_TOKEN") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("TELEGRAM_BOT_TOKEN is required");
            std::process::exit(1);
        }
    };
    let slop_law = env::var("OPENCHAT_TELEGRAM_SLOP_LAW")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    if let Err(error) = run(token, slop_law) {
        eprintln!("telegram bot failed: {error}");
        std::process::exit(1);
    }
}
