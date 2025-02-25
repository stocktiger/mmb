#![deny(
    non_ascii_idents,
    non_shorthand_field_patterns,
    no_mangle_generic_items,
    overflowing_literals,
    path_statements,
    unused_allocation,
    unused_comparisons,
    unused_parens,
    while_true,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_must_use,
    clippy::unwrap_used
)]

mod config;
mod handlers;
mod middleware;
mod routes;
mod server;
mod services;
mod ws;

use crate::config::load_config;
use crate::handlers::ws::ws_client;
use crate::server::start;
use crate::services::liquidity::LiquidityService;
use crate::ws::broker_messages::NewLiquidityDataMessage;
use casbin::{CoreApi, Enforcer};
use chrono::Duration;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    configure_logger();
    let config = load_config("config/base.toml");
    let enforcer = Enforcer::new("policy/model.conf", "policy/policy.csv")
        .await
        .expect("Failure to load enforcer policy");

    start(
        &config.address,
        "somesecretkey1".to_string(),
        "somesecretkey2".to_string(),
        Duration::days(1).num_seconds(),   // one day
        Duration::days(365).num_seconds(), // one year
        &config.database_url,
        enforcer,
        config.markets,
        config.refresh_data_interval_ms,
    )
    .await
}

fn configure_logger() {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Trace)
        .chain(std::io::stdout())
        .chain(fern::log_file("visualization.log").expect("Failure create log file"))
        .apply()
        .expect("Failure configure logger");
}
