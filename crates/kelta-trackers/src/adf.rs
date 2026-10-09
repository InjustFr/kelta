//! Atlassian Document Format ↔ Markdown (ARCHITECTURE §8.1).
//!
//! [`adf_to_markdown`] is a tolerant recursive walker: unknown nodes render their children (or
//! their `text`), malformed nodes are skipped, nothing here can fail or panic.
//! [`markdown_to_adf`] builds the comment body Jira Cloud expects: one paragraph per line.

use serde_json::{Value, json};

/// Render an ADF document (or any ADF node) as Markdown. A bare JSON string is returned as is.
pub fn adf_to_markdown(doc: &Value) -> String {
    match doc {
        Value::String(s) => s.clone(),
        Value::Object(_) => block(doc).trim().to_owned(),
        Value::Array(nodes) => blocks(nodes).trim().to_owned(),
        _ => String::new(),
    }
}

/// `{"type":"doc","version":1,"content":[paragraph per line]}`; empty lines become empty paragraphs.
pub fn markdown_to_adf(md: &str) -> Value {
    let content: Vec<Value> = md
        .lines()
        .map(|line| {
            if line.is_empty() {
                json!({"type": "paragraph", "content": []})
            } else {
                json!({"type": "paragraph", "content": [{"type": "text", "text": line}]})
            }
        })
        .collect();
    json!({"type": "doc", "version": 1, "content": content})
}

fn node_type(n: &Value) -> &str {
    n.get("type").and_then(Value::as_str).unwrap_or("")
}

fn children(n: &Value) -> &[Value] {
    n.get("content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn attr_str<'a>(n: &'a Value, key: &str) -> Option<&'a str> {
    n.get("attrs").and_then(|a| a.get(key)).and_then(Value::as_str)
}

fn is_inline(t: &str) -> bool {
    matches!(
        t,
        "text"
            | "hardBreak"
            | "mention"
            | "emoji"
            | "inlineCard"
            | "date"
            | "status"
            | "mediaInline"
            | "placeholder"
    )
}

/// Block-level children joined by blank lines.
fn blocks(nodes: &[Value]) -> String {
    let parts: Vec<String> = nodes
        .iter()
        .map(|n| if is_inline(node_type(n)) { inline(n) } else { block(n) })
        .filter(|s| !s.trim().is_empty())
        .collect();
    parts.join("\n\n")
}

fn inlines(nodes: &[Value]) -> String {
    nodes
        .iter()
        .map(|n| {
            if is_inline(node_type(n)) || n.get("text").is_some() {
                inline(n)
            } else {
                // A block where an inline was expected: keep its text on one line.
                block(n).replace("\n\n", " ").replace('\n', " ")
            }
        })
        .collect()
}

fn block(n: &Value) -> String {
    match node_type(n) {
        "doc" | "listItem" | "nestedExpandBody" => blocks(children(n)),
        "paragraph" => inlines(children(n)),
        "heading" => {
            let level =
                n.get("attrs").and_then(|a| a.get("level")).and_then(Value::as_u64).unwrap_or(1).clamp(1, 6);
            format!("{} {}", "#".repeat(level as usize), inlines(children(n)))
        }
        "bulletList" => list(n, |_| "- ".to_owned()),
        "orderedList" => {
            let start = n.get("attrs").and_then(|a| a.get("order")).and_then(Value::as_u64).unwrap_or(1);
            list(n, move |i| format!("{}. ", start + i as u64))
        }
        "taskList" => list(n, |_| String::new()),
        "decisionList" => list(n, |_| "- ".to_owned()),
        "taskItem" => {
            let done = attr_str(n, "state") == Some("DONE");
            format!("- [{}] {}", if done { "x" } else { " " }, inline_or_blocks(children(n)))
        }
        "decisionItem" => format!("- {}", inline_or_blocks(children(n))),
        "codeBlock" => {
            let lang = attr_str(n, "language").unwrap_or("");
            let text: String =
                children(n).iter().filter_map(|c| c.get("text").and_then(Value::as_str)).collect();
            format!("```{lang}\n{text}\n```")
        }
        "blockquote" => quote(&blocks(children(n))),
        "panel" => quote(&blocks(children(n))),
        "rule" => "---".to_owned(),
        "table" => table(n),
        "mediaSingle" | "mediaGroup" => children(n).iter().map(media).collect::<Vec<_>>().join("\n\n"),
        "media" => media(n),
        "expand" | "nestedExpand" => {
            let title = attr_str(n, "title").unwrap_or("");
            let body = blocks(children(n));
            if title.is_empty() { body } else { format!("**{title}**\n\n{body}") }
        }
        "" => String::new(),
        // Unknown node: never fail, show whatever it holds.
        _ => {
            if let Some(t) = n.get("text").and_then(Value::as_str) {
                t.to_owned()
            } else {
                blocks(children(n))
            }
        }
    }
}

/// `taskItem` / `decisionItem` children are inline nodes or paragraphs.
fn inline_or_blocks(nodes: &[Value]) -> String {
    if nodes.iter().all(|c| is_inline(node_type(c))) {
        inlines(nodes)
    } else {
        blocks(nodes).replace("\n\n", " ")
    }
}

fn list(n: &Value, marker: impl Fn(usize) -> String) -> String {
    let mut lines = Vec::new();
    for (i, item) in children(n).iter().enumerate() {
        let t = node_type(item);
        let body = match t {
            "taskItem" | "decisionItem" => block(item),
            "listItem" => {
                let m = marker(i);
                let inner = tight(children(item));
                let pad = " ".repeat(m.chars().count());
                let mut it = inner.lines();
                let first = it.next().unwrap_or("");
                let mut s = format!("{m}{first}");
                for l in it {
                    s.push('\n');
                    if !l.is_empty() {
                        s.push_str(&pad);
                    }
                    s.push_str(l);
                }
                s
            }
            // A list inside a list (some producers do that) or an unknown item.
            _ => format!("{}{}", marker(i), block(item)),
        };
        lines.push(body);
    }
    lines.join("\n")
}

/// List item content: children on consecutive lines (a nested list stays attached to its item).
fn tight(nodes: &[Value]) -> String {
    let parts: Vec<String> = nodes
        .iter()
        .map(|n| if is_inline(node_type(n)) { inline(n) } else { block(n) })
        .filter(|s| !s.trim().is_empty())
        .collect();
    parts.join("\n")
}

fn quote(inner: &str) -> String {
    inner
        .lines()
        .map(|l| if l.is_empty() { ">".to_owned() } else { format!("> {l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cell_text(cell: &Value) -> String {
    blocks(children(cell)).replace("\n\n", "<br>").replace('\n', "<br>").replace('|', "\\|")
}

fn table(n: &Value) -> String {
    let rows: Vec<Vec<String>> = children(n)
        .iter()
        .filter(|r| node_type(r) == "tableRow")
        .map(|r| children(r).iter().map(cell_text).collect())
        .collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let line = |cells: &Vec<String>| {
        let mut c = cells.clone();
        c.resize(width, String::new());
        format!("| {} |", c.join(" | "))
    };
    let mut out = vec![line(&rows[0]), format!("| {} |", vec!["---"; width].join(" | "))];
    out.extend(rows.iter().skip(1).map(line));
    out.join("\n")
}

fn media(n: &Value) -> String {
    let alt = attr_str(n, "alt").or_else(|| attr_str(n, "id")).unwrap_or("attachment");
    match attr_str(n, "url") {
        Some(url) => format!("![{alt}]({url})"),
        None => format!("[attachment: {alt}]"),
    }
}

fn inline(n: &Value) -> String {
    match node_type(n) {
        "text" => text_node(n),
        "hardBreak" => "  \n".to_owned(),
        "mention" => attr_str(n, "text").map(str::to_owned).unwrap_or_else(|| "@user".to_owned()),
        "emoji" => attr_str(n, "text")
            .map(str::to_owned)
            .or_else(|| attr_str(n, "shortName").map(str::to_owned))
            .unwrap_or_default(),
        "inlineCard" | "blockCard" | "embedCard" => {
            attr_str(n, "url").map(|u| format!("<{u}>")).unwrap_or_default()
        }
        "date" => {
            let ms = attr_str(n, "timestamp").and_then(|t| t.parse::<i64>().ok());
            ms.and_then(|ms| time::OffsetDateTime::from_unix_timestamp(ms / 1000).ok())
                .map(|d| format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()))
                .unwrap_or_default()
        }
        "status" => attr_str(n, "text").map(|t| format!("[{t}]")).unwrap_or_default(),
        "placeholder" => attr_str(n, "text").unwrap_or("").to_owned(),
        "mediaInline" => media(n),
        _ => {
            if let Some(t) = n.get("text").and_then(Value::as_str) {
                t.to_owned()
            } else {
                inlines(children(n))
            }
        }
    }
}

fn text_node(n: &Value) -> String {
    let mut text = n.get("text").and_then(Value::as_str).unwrap_or("").to_owned();
    let marks = n.get("marks").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    for m in marks {
        match node_type(m) {
            "code" => text = wrap(&text, "`"),
            "strong" => text = wrap(&text, "**"),
            "em" => text = wrap(&text, "*"),
            "strike" => text = wrap(&text, "~~"),
            "link" => {
                if let Some(href) = attr_str(m, "href") {
                    text = format!("[{text}]({href})");
                }
            }
            _ => {}
        }
    }
    text
}

/// Put `marker` around the trimmed text, keeping edge whitespace outside (CommonMark).
fn wrap(text: &str, marker: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_owned();
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    format!("{lead}{marker}{trimmed}{marker}{trail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paragraph_and_marks() {
        let doc = json!({"type":"doc","version":1,"content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"Hello "},
                {"type":"text","text":"world ","marks":[{"type":"strong"}]},
                {"type":"text","text":"link","marks":[{"type":"link","attrs":{"href":"https://e.com"}}]}
            ]}
        ]});
        assert_eq!(adf_to_markdown(&doc), "Hello **world** [link](https://e.com)");
    }

    #[test]
    fn unknown_nodes_render_children_and_never_fail() {
        let doc = json!({"type":"doc","content":[
            {"type":"futureWidget","content":[{"type":"paragraph","content":[{"type":"text","text":"inside"}]}]},
            {"type":"mystery","text":"raw text"},
            42, null, {"content": "not an array"}
        ]});
        assert_eq!(adf_to_markdown(&doc), "inside\n\nraw text");
        assert_eq!(adf_to_markdown(&Value::Null), "");
        assert_eq!(adf_to_markdown(&json!("wiki text")), "wiki text");
    }

    #[test]
    fn comment_body_is_one_paragraph_per_line() {
        let v = markdown_to_adf("one\n\ntwo");
        let c = v["content"].as_array().unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0]["content"][0]["text"], "one");
        assert_eq!(c[1]["content"].as_array().unwrap().len(), 0);
        assert_eq!(adf_to_markdown(&v), "one\n\ntwo");
    }
}
