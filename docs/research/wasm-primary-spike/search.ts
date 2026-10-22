// Throwaway guest: same typed WIT query as the earlier Rust smoke test.
type SearchResult = { id: string; title: string };

export function query(text: string): SearchResult[] {
  return [
    { id: "apps", title: "Open Applications" },
    { id: "calculator", title: "Calculator" },
    { id: "links", title: "Quicklinks" },
  ].filter((item) => item.title.toLowerCase().includes(text.toLowerCase()));
}
