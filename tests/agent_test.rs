use ceo::agent::{AgentKind};
use ceo::config::AgentConfig;

#[test]
fn agent_kind_from_config_claude() {
    let config = AgentConfig {
        agent_type: "claude".to_string(),
        command: "claude".to_string(),
        args: vec!["-p".to_string()],
        timeout_secs: 120,
    };
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Claude(_)));
}

#[test]
fn agent_kind_from_config_codex() {
    let config = AgentConfig {
        agent_type: "codex".to_string(),
        command: "codex".to_string(),
        args: vec![],
        timeout_secs: 120,
    };
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Codex(_)));
}

#[test]
fn agent_kind_from_config_unknown_falls_back_to_generic() {
    let config = AgentConfig {
        agent_type: "llama".to_string(),
        command: "llama-cli".to_string(),
        args: vec!["--prompt".to_string()],
        timeout_secs: 60,
    };
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Generic(_)));
}

#[test]
fn agent_kind_default_config_is_claude() {
    let config = AgentConfig::default();
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Claude(_)));
}
