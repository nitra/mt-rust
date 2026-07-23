//! Порожній крейт-заглушка: єдине призначення — smoke-тест нижче, що
//! доводить резолвинг git-залежності `mt-protocol` (`nitra/mt`).

#[cfg(test)]
mod tests {
    #[test]
    fn mt_protocol_git_dependency_resolves_and_docs_are_reachable() {
        let content = mt_protocol::get("index.md")
            .expect("mt-protocol::get(\"index.md\") має повернути вміст із вшитого docs/ корпусу");
        assert!(!content.is_empty(), "index.md не має бути порожнім");
    }
}
