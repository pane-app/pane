//! Throwaway component smoke test; not the proposed production SDK.
wit_bindgen::generate!({ path: "wit", world: "search-extension" });

struct Extension;

impl Guest for Extension {
    fn query(text: String) -> Vec<SearchResult> {
        [
            ("apps", "Open Applications"),
            ("calculator", "Calculator"),
            ("links", "Quicklinks"),
        ]
        .into_iter()
        .filter(|(_, title)| title.to_lowercase().contains(&text.to_lowercase()))
        .map(|(id, title)| SearchResult { id: id.into(), title: title.into() })
        .collect()
    }
}

export!(Extension);
