//! Node-Cronflow Core Engine
//! 
//! This is the Rust core engine that handles state management, job execution,
//! and communication with the Node.js SDK via N-API.

pub mod error;
pub mod models;
pub mod database;
pub mod state;
pub mod bridge;

/// Core engine version
pub const VERSION: &str = "0.1.0";

/// Initialize the core engine
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();
    
    log::info!("Node-Cronflow Core Engine v{} initialized", VERSION);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }

    #[test]
    fn test_init() {
        assert!(init().is_ok());
    }
}
