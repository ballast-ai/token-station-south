//! Declared model-parameter words that a host validates, exported under a path that names no
//! provider.
//!
//! A host checks a model row's `supported_parameters` against the words a component honours (a
//! misspelt word in a reserved namespace is refused at save time). The words themselves belong to
//! the component that reads them; this module only re-exports them, so a host can import the list
//! without naming a provider module.

/// The prompt-cache checkpoint words a model row can declare
/// ([`crate::reference_bedrock_converse::CACHE_POINT_PARAMETERS`]). They share one reserved
/// namespace, the prefix every word starts with.
pub const CACHE_POINT_PARAMETERS: [&str; 3] =
    crate::reference_bedrock_converse::CACHE_POINT_PARAMETERS;

#[cfg(test)]
mod tests {
    use super::CACHE_POINT_PARAMETERS;

    #[test]
    fn the_cache_point_words_share_one_namespace() {
        let namespace = CACHE_POINT_PARAMETERS[0]
            .rsplit_once('.')
            .map(|(head, _)| head)
            .expect("a dotted word");
        assert!(
            CACHE_POINT_PARAMETERS.iter().all(|word| word.starts_with(namespace)),
            "{CACHE_POINT_PARAMETERS:?}"
        );
    }
}
