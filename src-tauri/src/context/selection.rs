use crate::domain::{ContextDocument, SelectedContext};

use super::loader::{ContextPack, normalize_words};

#[must_use]
pub fn select_context(pack: &ContextPack, user_text: &str) -> SelectedContext {
    let input_words = normalize_words(user_text);
    let documents = pack
        .documents
        .iter()
        .filter(|document| {
            document.always_include
                || document
                    .keywords
                    .iter()
                    .any(|keyword| contains_phrase(&input_words, keyword))
        })
        .map(|document| ContextDocument {
            id: document.id.clone(),
            title: document.title.clone(),
            content: document.content.clone(),
        })
        .collect();

    SelectedContext {
        pack_id: pack.id().to_owned(),
        documents,
    }
}

fn contains_phrase(input_words: &[String], phrase: &[String]) -> bool {
    !phrase.is_empty()
        && input_words
            .windows(phrase.len())
            .any(|window| window == phrase)
}
