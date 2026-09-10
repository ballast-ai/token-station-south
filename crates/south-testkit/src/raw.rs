//! Owned builders for the raw call shapes, so host tests stop hand-rolling them.

use std::fmt;

use south_contracts::{ControlledUserAgentV1, QueryStringV1, SignedHeaderSetV1, SignedHeaderV1};
use south_core::raw::{
    RawAuthV1, RawGetProviderCallV1, RawMultipartProviderCallV1, RawProviderCallV1,
    RawSignedProviderCallV1,
};

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

/// Owns the data behind one [`RawSignedProviderCallV1`] and lends borrowed views of it.
///
/// The signed twin of [`RawProviderCallBuilderV1`]: the same defaults, with a three-header
/// declaration (`authorization`, `x-amz-date`, `x-amz-content-sha256` — a session-token-less
/// `SigV4` signer's set) in place of the credential arm.
pub struct RawSignedProviderCallBuilderV1 {
    endpoint: String,
    relative_path: String,
    bound_slot: String,
    requested_slot: String,
    headers: Vec<(String, String)>,
    body: String,
    emits: SignedHeaderSetV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
}

impl RawSignedProviderCallBuilderV1 {
    /// Creates a builder holding a minimal valid host-signed call.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: "https://provider.invalid".to_owned(),
            relative_path: "model/invoke".to_owned(),
            bound_slot: "primary".to_owned(),
            requested_slot: "primary".to_owned(),
            headers: Vec::new(),
            body: "{}".to_owned(),
            emits: SignedHeaderSetV1::new(&[
                SignedHeaderV1::Authorization,
                SignedHeaderV1::XAmzDate,
                SignedHeaderV1::XAmzContentSha256,
            ])
            .unwrap_or_else(|_| unreachable!("three distinct permitted headers form a valid set")),
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

    /// Replaces the finalizer's declaration.
    #[must_use]
    pub fn emits(mut self, emits: SignedHeaderSetV1) -> Self {
        self.emits = emits;
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

    /// Lends the borrowed host-signed raw call the signed entry points consume.
    #[must_use]
    pub fn as_raw_signed_call(&self) -> RawSignedProviderCallV1<'_> {
        RawSignedProviderCallV1 {
            endpoint: &self.endpoint,
            relative_path: &self.relative_path,
            bound_slot: &self.bound_slot,
            requested_slot: &self.requested_slot,
            headers: &self.headers,
            body: &self.body,
            emits: &self.emits,
            query: self.query.clone(),
            user_agent: self.user_agent,
        }
    }
}

impl Default for RawSignedProviderCallBuilderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RawSignedProviderCallBuilderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSignedProviderCallBuilderV1")
            .field("declared_header_count", &self.emits.len())
            .field("header_count", &self.headers.len())
            .field("has_query", &self.query.is_some())
            .field("has_user_agent", &self.user_agent.is_some())
            .finish_non_exhaustive()
    }
}

/// Owns the data behind one [`RawGetProviderCallV1`] and lends borrowed views of it.
///
/// The body-less twin of [`RawProviderCallBuilderV1`] (HTTP contract version six): the same
/// defaults minus the body, with a task-polling path in place of the chat one. There is no
/// `body` setter because the raw GET has no body field.
pub struct RawGetProviderCallBuilderV1 {
    endpoint: String,
    relative_path: String,
    bound_slot: String,
    requested_slot: String,
    headers: Vec<(String, String)>,
    auth: RawAuthV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
}

impl RawGetProviderCallBuilderV1 {
    /// Creates a builder holding a minimal valid Bearer GET.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: "https://provider.invalid".to_owned(),
            relative_path: "v1/videos/276843862449040".to_owned(),
            bound_slot: "primary".to_owned(),
            requested_slot: "primary".to_owned(),
            headers: Vec::new(),
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

    /// Lends the borrowed raw GET the buffered GET entry point consumes.
    #[must_use]
    pub fn as_raw_get_call(&self) -> RawGetProviderCallV1<'_> {
        RawGetProviderCallV1 {
            endpoint: &self.endpoint,
            relative_path: &self.relative_path,
            bound_slot: &self.bound_slot,
            requested_slot: &self.requested_slot,
            headers: &self.headers,
            auth: self.auth,
            query: self.query.clone(),
            user_agent: self.user_agent,
        }
    }
}

impl Default for RawGetProviderCallBuilderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RawGetProviderCallBuilderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawGetProviderCallBuilderV1")
            .field("auth", &self.auth)
            .field("header_count", &self.headers.len())
            .field("has_query", &self.query.is_some())
            .field("has_user_agent", &self.user_agent.is_some())
            .finish_non_exhaustive()
    }
}

/// Owns the data behind one [`RawMultipartProviderCallV1`] and lends borrowed views of it.
///
/// The multipart twin of [`RawProviderCallBuilderV1`] (HTTP contract version seven): the same
/// defaults with opaque bytes and their boundary in place of the JSON body, and no `header`
/// escape hatch for `content-type` — that shape renders its own, and a builder that let a test
/// set one would let a test build a request the contract refuses.
pub struct RawMultipartProviderCallBuilderV1 {
    endpoint: String,
    relative_path: String,
    bound_slot: String,
    requested_slot: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    boundary: String,
    auth: RawAuthV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
}

impl RawMultipartProviderCallBuilderV1 {
    /// Creates a builder holding a minimal valid Bearer multipart call.
    ///
    /// The default body is one text part delimited by the default boundary, so a test that only
    /// cares about the auth arm or the destination states exactly that.
    #[must_use]
    pub fn new() -> Self {
        let boundary = "south-testkit-boundary".to_owned();
        Self {
            endpoint: "https://provider.invalid".to_owned(),
            relative_path: "v1/audio/transcriptions".to_owned(),
            bound_slot: "primary".to_owned(),
            requested_slot: "primary".to_owned(),
            headers: Vec::new(),
            body: default_multipart_body(&boundary),
            boundary,
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
    ///
    /// A `content-type` passed here is retained verbatim so a test can prove the contract
    /// refuses it; the builder does not silently drop it.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Replaces the boundary **and** rebuilds the default body around it, so the pair stays
    /// consistent unless a test deliberately breaks it with [`Self::body`].
    #[must_use]
    pub fn boundary(mut self, boundary: impl Into<String>) -> Self {
        self.boundary = boundary.into();
        self.body = default_multipart_body(&self.boundary);
        self
    }

    /// Replaces the multipart body bytes, leaving the boundary alone.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
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

    /// Lends the borrowed raw multipart call the buffered entry point consumes.
    #[must_use]
    pub fn as_raw_multipart_call(&self) -> RawMultipartProviderCallV1<'_> {
        RawMultipartProviderCallV1 {
            endpoint: &self.endpoint,
            relative_path: &self.relative_path,
            bound_slot: &self.bound_slot,
            requested_slot: &self.requested_slot,
            headers: &self.headers,
            body: &self.body,
            boundary: &self.boundary,
            auth: self.auth,
            query: self.query.clone(),
            user_agent: self.user_agent,
        }
    }
}

/// One text part delimited by `boundary` — the smallest body that satisfies the contract.
fn default_multipart_body(boundary: &str) -> Vec<u8> {
    format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nm\r\n--{boundary}--\r\n"
    )
    .into_bytes()
}

impl Default for RawMultipartProviderCallBuilderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RawMultipartProviderCallBuilderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawMultipartProviderCallBuilderV1")
            .field("auth", &self.auth)
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
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
    fn default_signed_builder_lends_a_minimal_valid_host_signed_call() {
        use south_contracts::ProviderAuthV1;
        use south_core::raw::{parse_raw_signed_call, raw_signed_call_parses};

        let builder = RawSignedProviderCallBuilderV1::new().slot("aws.primary");
        let raw = builder.as_raw_signed_call();
        assert!(raw_signed_call_parses(&raw));
        let (_, request) = parse_raw_signed_call(&raw).expect("parses");
        assert!(
            matches!(request.auth(), ProviderAuthV1::HostSigned { emits, .. } if emits.len() == 3)
        );
    }

    #[test]
    fn default_get_builder_lends_a_minimal_valid_bearer_get() {
        use south_core::raw::{parse_raw_get_call, raw_get_call_parses};

        let builder = RawGetProviderCallBuilderV1::new();
        let raw = builder.as_raw_get_call();
        assert!(raw_get_call_parses(&raw));
        assert!(matches!(raw.auth, RawAuthV1::Bearer));
        let (_, request) = parse_raw_get_call(&raw).expect("parses");
        assert_eq!(request.relative_path().as_str(), "v1/videos/276843862449040");
        assert!(request.query().is_none());

        let query = QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, "7")]).unwrap();
        let builder = RawGetProviderCallBuilderV1::new()
            .relative_path("v1/query/video_generation")
            .slot("minimax.primary")
            .header("accept", "application/json")
            .auth(RawAuthV1::HeaderSecret(SecretHeaderV1::XApiKey))
            .query(query.clone());
        let raw = builder.as_raw_get_call();
        assert_eq!(raw.bound_slot, "minimax.primary");
        assert_eq!(raw.headers, [("accept".to_owned(), "application/json".to_owned())]);
        let (_, request) = parse_raw_get_call(&raw).expect("parses");
        assert_eq!(request.query(), Some(&query));
        assert!(matches!(request.auth(), south_contracts::ProviderAuthV1::HeaderSecret { .. }));
        let rendered = format!("{builder:?}");
        assert!(!rendered.contains("minimax.primary"));
        assert!(!rendered.contains("video_generation"));
    }

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
