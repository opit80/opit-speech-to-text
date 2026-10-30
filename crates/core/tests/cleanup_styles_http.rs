use opit_core::cleanup::{self, CleanupClient, CleanupConfig};
use opit_core::rules::{RulePack, RuleSet};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn named_style_resolves_model_and_language_prompt_with_shared_glossary() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model":"formal-model"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"choices":[{"message":{"content":"Edited result."}}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut config = CleanupConfig {
        enabled: true,
        base_url: server.uri(),
        api_key_ref: None,
        active_style_id: "formal".into(),
        ..CleanupConfig::default()
    };
    config.styles.iter_mut().find(|s| s.id == "formal").unwrap().model = Some("formal-model".into());
    let rules =
        RuleSet::compile(&[RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [Opit]\n").unwrap()]).0;
    let prompt = cleanup::system_prompt(&config, "tr", &rules, "Shared context");
    assert!(prompt.contains(&cleanup::styles::builtin_prompt("formal", "tr")));
    assert!(prompt.contains("Opit"));
    assert!(prompt.contains("Shared context"));
    let client = CleanupClient::new(config, None).unwrap();
    assert!(client.clean("Some dictated result.", &prompt).await.is_ok());
}
