//! Starts Brook.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("brook=info,brook_core=info"))
        .init();
    brook::run();
}
