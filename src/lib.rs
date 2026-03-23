pub mod config;
pub mod discovery;
pub mod middleware;
pub mod proxy;
pub mod routes;
pub mod server;
pub mod types;

pub use config::*;

#[cfg(test)]
mod lib_tests {
    use super::*;

    #[test]
    fn test_config_module_exports() {
        // Verify that config module is properly exported
        let config = Config::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8765);
    }

    #[test]
    fn test_module_structure() {
        // Verify that all modules are accessible
        let _ = config::Config::default();
    }
}
