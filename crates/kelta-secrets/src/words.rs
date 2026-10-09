//! POSIX-style word splitting for `command:<argv>` refs (no shell is ever involved).

/// Split into argv: whitespace separates words, `'…'` is literal, `"…"` honours `\" \\ \$ \``,
/// a backslash outside quotes escapes the next character.
pub fn split(input: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err("unterminated single quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(n @ ('"' | '\\' | '$' | '`')) => cur.push(n),
                            Some(n) => {
                                cur.push('\\');
                                cur.push(n);
                            }
                            None => return Err("unterminated double quote".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("unterminated double quote".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(n) => cur.push(n),
                    None => return Err("dangling backslash".into()),
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn splits_like_a_posix_shell_without_expanding() {
        assert_eq!(split("pass show jira/acme").unwrap(), ["pass", "show", "jira/acme"]);
        assert_eq!(
            split("op read 'op://Private/Jira token/credential'").unwrap(),
            ["op", "read", "op://Private/Jira token/credential"]
        );
        assert_eq!(split(r#"echo "a \"b\" $HOME" c\ d"#).unwrap(), ["echo", r#"a "b" $HOME"#, "c d"]);
        assert_eq!(split("  a   ''  b ").unwrap(), ["a", "", "b"]);
        assert!(split("echo 'oops").is_err());
        assert!(split("echo \"oops").is_err());
        assert!(split("echo \\").is_err());
        assert!(split("   ").unwrap().is_empty());
    }
}
