use ceo::config::Config;

#[test]
fn parse_full_config() {
    let toml_str = r#"
[agent]
command = "my-agent"
args = ["--verbose", "--model", "opus"]
timeout_secs = 300

[[repos]]
name = "acme/frontend"
labels_required = ["ready-for-ai", "bug"]

[[repos]]
name = "acme/backend"
labels_required = ["ai-ok"]

[[team]]
github = "alice"
name = "Alice Smith"
role = "lead"

[[team]]
github = "bob"
name = "Bob Jones"
role = "reviewer"
"#;

    let config: Config = toml::from_str(toml_str).unwrap();

    // Agent
    assert_eq!(config.agent.command, "my-agent");
    assert_eq!(config.agent.args, vec!["--verbose", "--model", "opus"]);
    assert_eq!(config.agent.timeout_secs, 300);

    // Repos
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[0].name, "acme/frontend");
    assert_eq!(
        config.repos[0].labels_required,
        vec!["ready-for-ai", "bug"]
    );
    assert_eq!(config.repos[1].name, "acme/backend");
    assert_eq!(config.repos[1].labels_required, vec!["ai-ok"]);

    // Team
    assert_eq!(config.team.len(), 2);
    assert_eq!(config.team[0].github, "alice");
    assert_eq!(config.team[0].name, "Alice Smith");
    assert_eq!(config.team[0].role, "lead");
    assert_eq!(config.team[1].github, "bob");
    assert_eq!(config.team[1].name, "Bob Jones");
    assert_eq!(config.team[1].role, "reviewer");
}

#[test]
fn parse_minimal_config() {
    let toml_str = r#"
[[repos]]
name = "acme/app"
"#;

    let config: Config = toml::from_str(toml_str).unwrap();

    // Defaults for agent
    assert_eq!(config.agent.command, "claude");
    assert_eq!(config.agent.args, Vec::<String>::new());
    assert_eq!(config.agent.timeout_secs, 120);

    // Repos
    assert_eq!(config.repos.len(), 1);
    assert_eq!(config.repos[0].name, "acme/app");
    assert_eq!(config.repos[0].labels_required, Vec::<String>::new());

    // Team defaults to empty
    assert!(config.team.is_empty());
}

#[test]
fn config_load_from_string() {
    let toml_str = r#"
[agent]
command = "test-agent"
timeout_secs = 60

[[repos]]
name = "org/repo"
labels_required = ["approved"]

[[team]]
github = "dev1"
name = "Developer One"
"#;

    let config = Config::load_from_str(toml_str).unwrap();

    assert_eq!(config.agent.command, "test-agent");
    assert_eq!(config.agent.timeout_secs, 60);
    assert_eq!(config.agent.args, Vec::<String>::new());
    assert_eq!(config.repos.len(), 1);
    assert_eq!(config.repos[0].name, "org/repo");
    assert_eq!(config.repos[0].labels_required, vec!["approved"]);
    assert_eq!(config.team.len(), 1);
    assert_eq!(config.team[0].github, "dev1");
    assert_eq!(config.team[0].name, "Developer One");
    assert_eq!(config.team[0].role, "");
}

#[test]
fn parse_config_with_agent_type() {
    let toml_str = r#"
[agent]
type = "codex"
command = "codex"
args = ["-q"]
timeout_secs = 60

[[repos]]
name = "org/repo"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.agent_type, "codex");
    assert_eq!(config.agent.command, "codex");
}

#[test]
fn agent_type_defaults_to_claude() {
    let toml_str = r#"
[[repos]]
name = "org/repo"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.agent.agent_type, "claude");
}

#[test]
fn config_round_trip_serialize_deserialize() {
    let config = Config::load_from_str(r#"
[agent]
type = "claude"
command = "claude"
timeout_secs = 120

[[repos]]
name = "org/frontend"
labels_required = ["priority"]

[[team]]
github = "alice"
name = "Alice Smith"
role = "Lead"
"#).unwrap();

    let serialized = toml::to_string_pretty(&config).unwrap();
    let reparsed: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.agent.agent_type, "claude");
    assert_eq!(reparsed.repos[0].name, "org/frontend");
    assert_eq!(reparsed.team[0].github, "alice");
}

#[test]
fn config_get_field() {
    let config = Config::load_from_str(r#"
[agent]
type = "claude"
timeout_secs = 120

[[repos]]
name = "org/frontend"
"#).unwrap();

    assert_eq!(config.get_field("agent.type").unwrap(), "claude");
    assert_eq!(config.get_field("agent.timeout_secs").unwrap(), "120");
}

#[test]
fn config_set_agent_field() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    config.set_field("agent.type", "codex").unwrap();
    assert_eq!(config.agent.agent_type, "codex");

    config.set_field("agent.timeout_secs", "60").unwrap();
    assert_eq!(config.agent.timeout_secs, 60);

    config.set_field("agent.command", "/usr/bin/codex").unwrap();
    assert_eq!(config.agent.command, "/usr/bin/codex");

    config.set_field("agent.args", "-q,--verbose").unwrap();
    assert_eq!(config.agent.args, vec!["-q", "--verbose"]);
}

#[test]
fn config_set_repos_add_remove() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/existing"
"#).unwrap();

    config.set_field("repos.add", "org/new-repo").unwrap();
    assert_eq!(config.repos.len(), 2);
    assert_eq!(config.repos[1].name, "org/new-repo");

    config.set_field("repos.remove", "org/existing").unwrap();
    assert_eq!(config.repos.len(), 1);
    assert_eq!(config.repos[0].name, "org/new-repo");
}

#[test]
fn config_get_field_unknown_key_errors() {
    let config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    assert!(config.get_field("nonexistent.field").is_err());
}

#[test]
fn parse_config_with_models() {
    let config = Config::load_from_str(r#"
[agent]
type = "claude"
model = "sonnet"

[agent.models]
summary = "opus"
triage = "haiku"

[[repos]]
name = "org/repo"
"#).unwrap();

    assert_eq!(config.agent.model, "sonnet");
    assert_eq!(config.agent.model_for("summary"), "opus");
    assert_eq!(config.agent.model_for("triage"), "haiku");
    assert_eq!(config.agent.model_for("other"), "sonnet");
}

#[test]
fn config_get_set_model_fields() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    config.set_field("agent.model", "sonnet").unwrap();
    assert_eq!(config.get_field("agent.model").unwrap(), "sonnet");

    config.set_field("agent.models.triage", "haiku").unwrap();
    assert_eq!(config.get_field("agent.models.triage").unwrap(), "haiku");
    assert_eq!(config.agent.model_for("triage"), "haiku");
}

#[test]
fn parse_config_with_tools() {
    let config = Config::load_from_str(r#"
[agent]
type = "claude"

[agent.tools]
summary = []
triage = ["Bash(gh:*)", "Read"]

[[repos]]
name = "org/repo"
"#).unwrap();

    assert!(config.agent.tools_for("summary").unwrap().is_empty());
    assert_eq!(config.agent.tools_for("triage").unwrap(), &vec!["Bash(gh:*)".to_string(), "Read".to_string()]);
    assert!(config.agent.tools_for("other").is_none());
}

#[test]
fn config_get_set_tools_fields() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    config.set_field("agent.tools.triage", "Bash(gh:*),Read").unwrap();
    assert_eq!(config.get_field("agent.tools.triage").unwrap(), "Bash(gh:*),Read");
    assert_eq!(config.agent.tools_for("triage").unwrap(), &vec!["Bash(gh:*)".to_string(), "Read".to_string()]);
}

#[test]
fn parse_config_with_project() {
    let config = Config::load_from_str(r#"
[project]
org = "acme-corp"
number = 3

[[repos]]
name = "org/repo"
"#).unwrap();

    let project = config.project.unwrap();
    assert_eq!(project.org, "acme-corp");
    assert_eq!(project.number, 3);
}

#[test]
fn config_without_project_section() {
    let config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    assert!(config.project.is_none());
}

#[test]
fn config_get_set_project_fields() {
    let mut config = Config::load_from_str(r#"
[[repos]]
name = "org/repo"
"#).unwrap();

    config.set_field("project.org", "acme-corp").unwrap();
    config.set_field("project.number", "3").unwrap();

    assert_eq!(config.get_field("project.org").unwrap(), "acme-corp");
    assert_eq!(config.get_field("project.number").unwrap(), "3");
    assert_eq!(config.project.unwrap().number, 3);
}
