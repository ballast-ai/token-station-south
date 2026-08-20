//! An owned builder for [`RawProviderCallV1`], so host tests stop hand-rolling one.

use std::fmt;

use south_contracts::{ControlledUserAgentV1, QueryStringV1};
use south_core::raw::{RawAuthV1, RawProviderCallV1};

/// Owns the data behind one [`RawProviderCallV1`] and lends borrowed views of it.
///
/// The raw type is deliberately borrowed; host tests that assembled one by hand each carried the
/// same owned-backing boilerplate. Defaults form a minimal valid Bearer call so a test states
/// only what it is about.
pub struct RawProviderCallBuilderV1 {
    endpoint: String,
    relative_path: String,
    bound_slot: String,
    requested_slot: String,
    headers: Vec<(String, String)>,
    body: String,
    auth: RawAuthV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
}

impl RawProviderCallBuilderV1 {
    /// Creates a builder holding a minimal valid Bearer call.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: "https://provider.invalid".to_owned(),
            relative_path: "v1/chat/completions".to_owned(),
            bound_slot: "primary".to_owned(),
            requested_slot: "primary".to_owned(),
            headers: Vec::new(),
            body: "{}".to_owned(),
            auth: RawAuthV1::Bearer,
            query: None,
            user_agent: None,
        }
    }

    /// Replaces the trusted base endpoint.
    #[must_use]
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Replaces the provider-selected relative path.
    #[must_use]
    pub fn relative_path(mut self, relative_path: impl Into<String>) -> Self {
        self.relative_path = relative_path.into();
        self
    }

    /// Replaces the binding-side credential slot.
    #[must_use]
    pub fn bound_slot(mut self, bound_slot: impl Into<String>) -> Self {
        self.bound_slot = bound_slot.into();
        self
    }

    /// Replaces the request-declaration-side credential slot.
    #[must_use]
    pub fn requested_slot(mut self, requested_slot: impl Into<String>) -> Self {
        self.requested_slot = requested_slot.into();
        self
    }

    /// Replaces both slots with one value, the production shape.
    #[must_use]
    pub fn slot(self, slot: &str) -> Self {
        self.bound_slot(slot).requested_slot(slot)
    }

    /// Appends one ordinary request header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Replaces the JSON request body.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Replaces the authentication arm.
    #[must_use]
    pub const fn auth(mut self, auth: RawAuthV1) -> Self {
        self.auth = auth;
        self
    }

    /// Attaches a sanctioned query declaration.
    #[must_use]
    pub fn query(mut self, query: QueryStringV1) -> Self {
        self.query = Some(query);
        self
    }

    /// Attaches a sanctioned user-agent declaration.
    #[must_use]
    pub const fn user_agent(mut self, user_agent: ControlledUserAgentV1) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Lends the borrowed raw call the orchestration entry points consume.
    #[must_use]
    pub fn as_raw_call(&self) -> RawProviderCallV1<'_> {
        RawProviderCallV1 {
            endpoint: &self.endpoint,
            relative_path: &self.relative_path,
            bound_slot: &self.bound_slot,
            requested_slot: &self.requested_slot,
            headers: &self.headers,
            body: &self.body,
            auth: self.auth,
            query: self.query.clone(),
            user_agent: self.user_agent,
        }
    }
}

impl Default for RawProviderCallBuilderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RawProviderCallBuilderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawProviderCallBuilderV1")
            .field("auth", &self.auth)
            .field("header_count", &self.headers.len())
            .field("has_query", &self.query.is_some())
            .field("has_user_agent", &self.user_agent.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use south_contracts::{QueryParameterV1, QueryStringV1, SecretHeaderV1};
    use south_core::raw::{parse_raw_call, raw_call_parses};

    use super::*;

    #[test]
    fn default_builder_lends_a_minimal_valid_bearer_call() {
        let builder = RawProviderCallBuilderV1::new();
        let raw = builder.as_raw_call();
        assert!(raw_call_parses(&raw));
        assert!(matches!(raw.auth, RawAuthV1::Bearer));
    }

    #[test]
    fn builder_carries_every_customization_into_the_lent_call() {
        let query = QueryStringV1::try_from_iter([(QueryParameterV1::Alt, "sse")]).unwrap();
        let builder = RawProviderCallBuilderV1::new()
            .endpoint("https://alt.invalid/base")
            .relative_path("v1/messages")
            .slot("secondary")
            .header("x-request-id", "req-9")
            .body("{\"model\":\"m\"}")
            .auth(RawAuthV1::HeaderSecret(SecretHeaderV1::XApiKey))
            .query(query);

        let raw = builder.as_raw_call();
        assert_eq!(raw.endpoint, "https://alt.invalid/base");
        assert_eq!(raw.relative_path, "v1/messages");
        assert_eq!(raw.bound_slot, "secondary");
        assert_eq!(raw.requested_slot, "secondary");
        assert_eq!(raw.headers, [("x-request-id".to_owned(), "req-9".to_owned())]);
        assert_eq!(raw.body, "{\"model\":\"m\"}");

        let (_binding, request) = parse_raw_call(&raw).unwrap();
        assert!(request.query().is_some());
        match request.auth() {
            south_contracts::ProviderAuthV1::HeaderSecret { header, .. } => {
                assert_eq!(header.header_name(), "x-api-key");
            }
            _ => panic!("auth arm did not survive the builder round trip"),
        }
    }

    #[test]
    fn debug_output_does_not_leak_field_values() {
        let builder = RawProviderCallBuilderV1::new()
            .endpoint("https://sensitive.invalid")
            .body("{\"sentinel\":true}");
        let rendered = format!("{builder:?}");
        assert!(!rendered.contains("sensitive.invalid"));
        assert!(!rendered.contains("sentinel"));
    }
}
