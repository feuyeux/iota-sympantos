use super::*;
use iota_core::config::{BackendConfig, ModelConfig};

#[test]
fn test_is_api_key_configured_empty() {
    let config = NimiaConfig::default();
    assert!(!is_api_key_configured(&config, AcpBackend::Gemini));
    assert!(!is_api_key_configured(&config, AcpBackend::ClaudeCode));
}

#[test]
fn test_is_api_key_configured_placeholders() {
    let mut config = NimiaConfig::default();
    
    // Test with placeholder "<api-key>"
    let mut gemini_cfg = BackendConfig::default();
    let mut model_cfg = ModelConfig::default();
    model_cfg.api_key = Some("<api-key>".to_string());
    gemini_cfg.model = Some(model_cfg);
    config.gemini = Some(gemini_cfg);
    
    assert!(!is_api_key_configured(&config, AcpBackend::Gemini));

    // Test with placeholder "YOUR_API_KEY"
    let mut claude_cfg = BackendConfig::default();
    let mut model_cfg2 = ModelConfig::default();
    model_cfg2.api_key = Some("YOUR_API_KEY".to_string());
    claude_cfg.model = Some(model_cfg2);
    config.claude_code = Some(claude_cfg);
    
    assert!(!is_api_key_configured(&config, AcpBackend::ClaudeCode));
}

#[test]
fn test_is_api_key_configured_valid() {
    let mut config = NimiaConfig::default();
    let mut gemini_cfg = BackendConfig::default();
    let mut model_cfg = ModelConfig::default();
    model_cfg.api_key = Some("AIzaSyTestKey123".to_string());
    gemini_cfg.model = Some(model_cfg);
    config.gemini = Some(gemini_cfg);

    assert!(is_api_key_configured(&config, AcpBackend::Gemini));
}

#[test]
fn test_is_api_key_configured_env_fallback() {
    let config = NimiaConfig::default();
    
    // Set env var and check fallback
    unsafe {
        std::env::set_var("GEMINI_API_KEY", "env-key-test");
    }
    assert!(is_api_key_configured(&config, AcpBackend::Gemini));
    
    // Clean up
    unsafe {
        std::env::remove_var("GEMINI_API_KEY");
    }
}
