use ceo::agent::{AgentRunner, run_agent};

struct MockAgent {
    response: String,
}

impl AgentRunner for MockAgent {
    fn invoke(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }
}

#[test]
fn run_agent_returns_response() {
    let agent = MockAgent {
        response: "Summary: things happened.".to_string(),
    };
    let result = run_agent(&agent, "Summarize this").unwrap();
    assert_eq!(result, "Summary: things happened.");
}

#[test]
fn weekly_summary_prompt_contains_repo() {
    let prompt = ceo::agent::build_weekly_summary_prompt("org/frontend", "- #1 Fix bug\n- #2 Add feature");
    assert!(prompt.contains("org/frontend"));
    assert!(prompt.contains("Fix bug"));
}

#[test]
fn triage_prompt_contains_issue_info() {
    let prompt = ceo::agent::build_triage_prompt("Fix login redirect", "The login page redirects.", "alice: I think it's SSO.");
    assert!(prompt.contains("Fix login redirect"));
    assert!(prompt.contains("login page redirects"));
    assert!(prompt.contains("SSO"));
}
