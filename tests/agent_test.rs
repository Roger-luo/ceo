use std::collections::HashMap;
use ceo::agent::AgentKind;
use ceo::config::AgentConfig;

#[test]
fn agent_kind_from_config_claude() {
    let config = AgentConfig {
        agent_type: "claude".to_string(),
        command: "claude".to_string(),
        args: vec!["-p".to_string()],
        timeout_secs: 120,
        model: String::new(),
        models: HashMap::new(),
        tools: HashMap::new(),
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
        model: String::new(),
        models: HashMap::new(),
        tools: HashMap::new(),
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
        model: String::new(),
        models: HashMap::new(),
        tools: HashMap::new(),
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

#[test]
fn model_for_prompt_kind_with_override() {
    let config = AgentConfig {
        agent_type: "claude".to_string(),
        command: "claude".to_string(),
        args: vec![],
        timeout_secs: 120,
        model: "sonnet".to_string(),
        models: HashMap::from([("triage".to_string(), "haiku".to_string())]),
        tools: HashMap::new(),
    };
    assert_eq!(config.model_for("triage"), "haiku");
    assert_eq!(config.model_for("summary"), "sonnet");
    assert_eq!(config.model_for("unknown"), "sonnet");
}

#[test]
fn tools_for_prompt_kind() {
    let config = AgentConfig {
        agent_type: "claude".to_string(),
        command: "claude".to_string(),
        args: vec![],
        timeout_secs: 120,
        model: String::new(),
        models: HashMap::new(),
        tools: HashMap::from([
            ("triage".to_string(), vec!["Bash(gh:*)".to_string(), "Read".to_string()]),
        ]),
    };
    assert_eq!(config.tools_for("triage").unwrap(), &vec!["Bash(gh:*)".to_string(), "Read".to_string()]);
    assert!(config.tools_for("summary").is_none());
}
