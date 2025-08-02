// Simple test to verify configuration functionality
use rustypotato::config::Config;

fn main() {
    println!("Testing configuration system...");
    
    // Test default configuration
    let default_config = Config::default();
    println!("Default config created successfully");
    println!("Default port: {}", default_config.server.port);
    
    // Test validation
    match default_config.validate() {
        Ok(()) => println!("Default configuration is valid"),
        Err(e) => println!("Default configuration validation failed: {}", e),
    }
    
    // Test loading configuration (will use defaults since no file exists)
    match Config::load() {
        Ok(config) => {
            println!("Configuration loaded successfully");
            println!("Server port: {}", config.server.port);
            println!("AOF enabled: {}", config.storage.aof_enabled);
        }
        Err(e) => println!("Failed to load configuration: {}", e),
    }
    
    println!("Configuration system test completed!");
}