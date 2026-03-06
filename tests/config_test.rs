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
