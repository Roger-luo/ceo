use std::collections::HashMap;
use ceo::agent::AgentKind;
use ceo::config::{AgentConfig, ClaudeAgentConfig, CodexAgentConfig, GenericAgentConfig};

#[test]
fn agent_kind_from_config_claude() {
    let config = AgentConfig::Claude(ClaudeAgentConfig {
        command: "claude".to_string(),
        ..ClaudeAgentConfig::default()
    });
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Claude(_)));
}

#[test]
fn agent_kind_from_config_codex() {
    let config = AgentConfig::Codex(CodexAgentConfig::default());
    let agent = AgentKind::from_config(&config);
    assert!(matches!(agent, AgentKind::Codex(_)));
}

#[test]
fn agent_kind_from_config_unknown_falls_back_to_generic() {
    let config = AgentConfig::Generic(GenericAgentConfig {
        command: "llama-cli".to_string(),
        args: vec!["--prompt".to_string()],
        timeout_secs: 60,
    });
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
    let config = ClaudeAgentConfig {
        model: "sonnet".to_string(),
        models: HashMap::from([("triage".to_string(), "haiku".to_string())]),
        ..ClaudeAgentConfig::default()
    };
    assert_eq!(config.model_for("triage"), "haiku");
    assert_eq!(config.model_for("summary"), "sonnet");
    assert_eq!(config.model_for("unknown"), "sonnet");
}

#[test]
fn codex_model_for_prompt_kind() {
    let config = CodexAgentConfig {
        model: "o3".to_string(),
        models: HashMap::from([("triage".to_string(), "o4-mini".to_string())]),
        ..CodexAgentConfig::default()
    };
    assert_eq!(config.model_for("triage"), "o4-mini");
    assert_eq!(config.model_for("summary"), "o3");
}

#[test]
fn tools_for_prompt_kind() {
    let config = ClaudeAgentConfig {
        tools: HashMap::from([
            ("triage".to_string(), vec!["Bash(gh:*)".to_string(), "Read".to_string()]),
        ]),
        ..ClaudeAgentConfig::default()
    };
    assert_eq!(config.tools_for("triage").unwrap(), &vec!["Bash(gh:*)".to_string(), "Read".to_string()]);
    assert!(config.tools_for("summary").is_none());
}
