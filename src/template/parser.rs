use std::borrow::Cow;
use std::collections::HashMap;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    Literal(Cow<'a, str>),
    Tag(TagContent),
    ForStart(ForLoop),
    ForEnd,
}

impl Token<'_> {
    /// Take ownership of any borrowed data so the token outlives its source.
    pub fn into_owned(self) -> Token<'static> {
        match self {
            Token::Literal(s) => Token::Literal(Cow::Owned(s.into_owned())),
            Token::Tag(t) => Token::Tag(t),
            Token::ForStart(f) => Token::ForStart(f),
            Token::ForEnd => Token::ForEnd,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TagContent {
    pub name: String,
    pub params: HashMap<String, String>,
    pub pipe: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForLoop {
    pub var: String,
    pub iterable: Iterable,
    /// e.g. `limit:3`
    pub params: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Iterable {
    Range(i64, i64),
    Collection(String),
}

/// Tokenize a template into Literal, Tag, ForStart, and ForEnd tokens.
/// Literals borrow from the input to avoid allocation.
pub fn tokenize(input: &str) -> Result<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    let mut rest = input;

    while !rest.is_empty() {
        if let Some(tag_start) = rest.find("{%") {
            if tag_start > 0 {
                tokens.push(Token::Literal(Cow::Borrowed(&rest[..tag_start])));
            }

            let after_open = &rest[tag_start + 2..];
            let tag_end = after_open
                .find("%}")
                .ok_or_else(|| Error::Template("unclosed tag: missing '%}'".into()))?;

            let tag_body = after_open[..tag_end].trim();
            let token = parse_tag_body(tag_body)?;
            tokens.push(token);

            rest = &after_open[tag_end + 2..];
        } else {
            tokens.push(Token::Literal(Cow::Borrowed(rest)));
            break;
        }
    }

    Ok(tokens)
}

/// Parse the content between `{%` and `%}` into the right token type.
fn parse_tag_body(body: &str) -> Result<Token<'static>> {
    if body == "endfor" {
        return Ok(Token::ForEnd);
    }

    if let Some(stripped) = body.strip_prefix("for ") {
        return parse_for_loop(stripped.trim());
    }

    // Regular tag: name, optional params, optional pipe
    let (before_pipe, pipe) = split_pipe(body);
    let parts = tokenize_tag_parts(before_pipe);

    if parts.is_empty() {
        return Err(Error::Template("empty tag".into()));
    }

    let name = parts[0].to_string();
    let mut params = HashMap::new();

    for part in &parts[1..] {
        let (k, v) = parse_param(part)?;
        params.insert(k, v);
    }

    Ok(Token::Tag(TagContent { name, params, pipe }))
}

/// Parse for-loop: `i in (1..3)` or `i in memories limit:3`.
fn parse_for_loop(body: &str) -> Result<Token<'static>> {
    let parts: Vec<&str> = body.splitn(3, ' ').collect();
    if parts.len() < 3 || parts[1] != "in" {
        return Err(Error::Template(format!(
            "invalid for loop syntax: 'for {body}'"
        )));
    }

    let var = parts[0].to_string();
    let iterable_and_rest = parts[2].trim();

    // Range syntax: (start..end)
    if iterable_and_rest.starts_with('(') {
        let range_end = iterable_and_rest
            .find(')')
            .ok_or_else(|| Error::Template("unclosed range parenthesis".into()))?;

        let range_str = &iterable_and_rest[1..range_end];
        let range_parts: Vec<&str> = range_str.split("..").collect();
        if range_parts.len() != 2 {
            return Err(Error::Template(format!(
                "invalid range syntax: '{range_str}'"
            )));
        }

        let start: i64 = range_parts[0]
            .trim()
            .parse()
            .map_err(|_| Error::Template(format!("invalid range start: '{}'", range_parts[0])))?;
        let end: i64 = range_parts[1]
            .trim()
            .parse()
            .map_err(|_| Error::Template(format!("invalid range end: '{}'", range_parts[1])))?;

        Ok(Token::ForStart(ForLoop {
            var,
            iterable: Iterable::Range(start, end),
            params: HashMap::new(),
        }))
    } else {
        // Named collection with optional params
        let collection_parts = tokenize_tag_parts(iterable_and_rest);
        let collection_name = collection_parts
            .first()
            .ok_or_else(|| Error::Template("missing collection name in for loop".into()))?
            .to_string();

        let mut params = HashMap::new();
        for part in &collection_parts[1..] {
            let (k, v) = parse_param(part)?;
            params.insert(k, v);
        }

        Ok(Token::ForStart(ForLoop {
            var,
            iterable: Iterable::Collection(collection_name),
            params,
        }))
    }
}

/// Split a tag body into parts, respecting quoted strings.
fn tokenize_tag_parts(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

/// Split on the first `|` to separate tag body from pipe.
fn split_pipe(body: &str) -> (&str, Option<String>) {
    if let Some(pipe_pos) = body.find('|') {
        let before = body[..pipe_pos].trim();
        let after = body[pipe_pos + 1..].trim();
        if after.is_empty() {
            (before, None)
        } else {
            (before, Some(after.to_string()))
        }
    } else {
        (body, None)
    }
}

/// Parse a key=value or key:value parameter.
fn parse_param(param: &str) -> Result<(String, String)> {
    // `=` takes precedence over `:`
    let sep_pos = param.find('=').or_else(|| param.find(':'));

    match sep_pos {
        Some(pos) => {
            let key = param[..pos].to_string();
            let value = param[pos + 1..].to_string();
            if key.is_empty() {
                return Err(Error::Template(format!("empty parameter key in '{param}'")));
            }
            Ok((key, value))
        }
        None => Err(Error::Template(format!(
            "invalid parameter (missing '=' or ':'): '{param}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_passthrough() {
        let tokens = tokenize("hello world").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::Literal(Cow::Borrowed("hello world")));
    }

    #[test]
    fn test_simple_tag() {
        let tokens = tokenize("{% date %}").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::Tag(tag) => {
                assert_eq!(tag.name, "date");
                assert!(tag.params.is_empty());
                assert!(tag.pipe.is_none());
            }
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_tag_with_params() {
        let tokens = tokenize("{% date minus=1d %}").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::Tag(tag) => {
                assert_eq!(tag.name, "date");
                assert_eq!(tag.params.get("minus"), Some(&"1d".to_string()));
            }
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_mixed_content() {
        let tokens = tokenize("Hello {% custom:name %}, today is {% date %}").unwrap();
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0], Token::Literal(Cow::Borrowed("Hello ")));
        match &tokens[1] {
            Token::Tag(tag) => assert_eq!(tag.name, "custom:name"),
            _ => panic!("expected Tag"),
        }
        assert_eq!(tokens[2], Token::Literal(Cow::Borrowed(", today is ")));
        match &tokens[3] {
            Token::Tag(tag) => assert_eq!(tag.name, "date"),
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_for_range() {
        let tokens = tokenize("{% for i in (1..3) %}{% endfor %}").unwrap();
        assert_eq!(tokens.len(), 2);
        match &tokens[0] {
            Token::ForStart(fl) => {
                assert_eq!(fl.var, "i");
                assert_eq!(fl.iterable, Iterable::Range(1, 3));
            }
            _ => panic!("expected ForStart"),
        }
        assert_eq!(tokens[1], Token::ForEnd);
    }

    #[test]
    fn test_for_collection() {
        let tokens = tokenize("{% for i in memories limit:3 %}{% endfor %}").unwrap();
        assert_eq!(tokens.len(), 2);
        match &tokens[0] {
            Token::ForStart(fl) => {
                assert_eq!(fl.var, "i");
                assert_eq!(fl.iterable, Iterable::Collection("memories".into()));
                assert_eq!(fl.params.get("limit"), Some(&"3".to_string()));
            }
            _ => panic!("expected ForStart"),
        }
        assert_eq!(tokens[1], Token::ForEnd);
    }

    #[test]
    fn test_pipe() {
        let tokens = tokenize("{% i.result | summary %}").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::Tag(tag) => {
                assert_eq!(tag.name, "i.result");
                assert_eq!(tag.pipe, Some("summary".to_string()));
            }
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_empty_input() {
        let tokens = tokenize("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_tokenize_tag_parts_simple() {
        let result = tokenize_tag_parts("date minus=1d");
        assert_eq!(result, vec!["date", "minus=1d"]);
    }

    #[test]
    fn test_tokenize_tag_parts_quoted() {
        let result = tokenize_tag_parts("cmd arg=\"hello world\"");
        assert_eq!(result, vec!["cmd", "arg=\"hello world\""]);
    }

    #[test]
    fn test_tokenize_tag_parts_multiple_spaces() {
        let result = tokenize_tag_parts("date  minus=1d");
        assert_eq!(result, vec!["date", "minus=1d"]);
    }

    #[test]
    fn test_tokenize_tag_parts_tabs() {
        let result = tokenize_tag_parts("date\tminus=1d");
        assert_eq!(result, vec!["date", "minus=1d"]);
    }

    #[test]
    fn test_tokenize_tag_parts_empty() {
        let result = tokenize_tag_parts("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_tokenize_tag_parts_single() {
        let result = tokenize_tag_parts("date");
        assert_eq!(result, vec!["date"]);
    }

    #[test]
    fn test_split_pipe_with_pipe() {
        let (before, pipe) = split_pipe("i.result | summary");
        assert_eq!(before, "i.result");
        assert_eq!(pipe, Some("summary".to_string()));
    }

    #[test]
    fn test_split_pipe_no_pipe() {
        let (before, pipe) = split_pipe("date minus=1d");
        assert_eq!(before, "date minus=1d");
        assert_eq!(pipe, None);
    }

    #[test]
    fn test_split_pipe_empty_after() {
        let (before, pipe) = split_pipe("date |");
        assert_eq!(before, "date");
        assert_eq!(pipe, None);
    }

    #[test]
    fn test_split_pipe_whitespace() {
        let (before, pipe) = split_pipe("i.result|summary");
        assert_eq!(before, "i.result");
        assert_eq!(pipe, Some("summary".to_string()));
    }

    #[test]
    fn test_parse_param_equals() {
        let (key, value) = parse_param("minus=1d").unwrap();
        assert_eq!(key, "minus");
        assert_eq!(value, "1d");
    }

    #[test]
    fn test_parse_param_colon() {
        let (key, value) = parse_param("limit:3").unwrap();
        assert_eq!(key, "limit");
        assert_eq!(value, "3");
    }

    #[test]
    fn test_parse_param_equals_takes_precedence() {
        let (key, value) = parse_param("key=val:ue").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "val:ue");
    }

    #[test]
    fn test_parse_param_empty_key() {
        let err = parse_param("=value").unwrap_err();
        assert!(err.to_string().contains("empty parameter key"));
    }

    #[test]
    fn test_parse_param_no_separator() {
        let err = parse_param("nosep").unwrap_err();
        assert!(err.to_string().contains("missing '=' or ':'"));
    }

    #[test]
    fn test_parse_param_empty_value() {
        let (key, value) = parse_param("key=").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "");
    }

    #[test]
    fn test_for_loop_missing_in() {
        let err = tokenize("{% for i of (1..3) %}").unwrap_err();
        assert!(err.to_string().contains("invalid for loop"));
    }

    #[test]
    fn test_for_loop_unclosed_range() {
        let err = tokenize("{% for i in (1..3 %}").unwrap_err();
        assert!(err.to_string().contains("unclosed range"));
    }

    #[test]
    fn test_for_loop_invalid_range_start() {
        let err = tokenize("{% for i in (abc..3) %}").unwrap_err();
        assert!(err.to_string().contains("invalid range start"));
    }

    #[test]
    fn test_unclosed_tag() {
        let err = tokenize("{% date").unwrap_err();
        assert!(err.to_string().contains("unclosed tag"));
    }

    #[test]
    fn test_empty_tag() {
        let err = tokenize("{% %}").unwrap_err();
        assert!(err.to_string().contains("empty tag"));
    }

    #[test]
    fn test_nested_for_loops_parse() {
        let tokens =
            tokenize("{% for i in (1..2) %}{% for j in (3..4) %}{% endfor %}{% endfor %}").unwrap();
        assert_eq!(tokens.len(), 4);
        assert!(matches!(&tokens[0], Token::ForStart(fl) if fl.var == "i"));
        assert!(matches!(&tokens[1], Token::ForStart(fl) if fl.var == "j"));
        assert_eq!(tokens[2], Token::ForEnd);
        assert_eq!(tokens[3], Token::ForEnd);
    }

    #[test]
    fn test_for_loop_negative_range() {
        let tokens = tokenize("{% for i in (-3..-1) %}{% endfor %}").unwrap();
        assert_eq!(tokens.len(), 2);
        match &tokens[0] {
            Token::ForStart(fl) => {
                assert_eq!(fl.var, "i");
                assert_eq!(fl.iterable, Iterable::Range(-3, -1));
            }
            _ => panic!("expected ForStart"),
        }
    }

    #[test]
    fn test_for_loop_single_element_range() {
        let tokens = tokenize("{% for i in (5..5) %}{% endfor %}").unwrap();
        assert_eq!(tokens.len(), 2);
        match &tokens[0] {
            Token::ForStart(fl) => {
                assert_eq!(fl.iterable, Iterable::Range(5, 5));
            }
            _ => panic!("expected ForStart"),
        }
    }

    #[test]
    fn test_for_loop_invalid_range_end() {
        let err = tokenize("{% for i in (1..abc) %}").unwrap_err();
        assert!(err.to_string().contains("invalid range end"));
    }

    #[test]
    fn test_for_loop_missing_collection_name() {
        let err = tokenize("{% for i in %}").unwrap_err();
        assert!(err.to_string().contains("invalid for loop"));
    }

    #[test]
    fn test_tag_with_colon_separator_in_param() {
        let tokens = tokenize("{% date limit:5 %}").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::Tag(tag) => {
                assert_eq!(tag.name, "date");
                assert_eq!(tag.params.get("limit"), Some(&"5".to_string()));
            }
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_multiple_adjacent_tags() {
        let tokens = tokenize("{% date %}{% result %}").unwrap();
        assert_eq!(tokens.len(), 2);
        match &tokens[0] {
            Token::Tag(tag) => assert_eq!(tag.name, "date"),
            _ => panic!("expected Tag"),
        }
        match &tokens[1] {
            Token::Tag(tag) => assert_eq!(tag.name, "result"),
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_quoted_string_in_param_preserves_spaces() {
        let tokens = tokenize("{% cmd arg=\"hello world\" %}").unwrap();
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::Tag(tag) => {
                assert_eq!(tag.name, "cmd");
                assert_eq!(tag.params.get("arg"), Some(&"\"hello world\"".to_string()));
            }
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_only_whitespace_between_tags() {
        let tokens = tokenize("{% date %} {% result %}").unwrap();
        assert_eq!(tokens.len(), 3);
        match &tokens[0] {
            Token::Tag(tag) => assert_eq!(tag.name, "date"),
            _ => panic!("expected Tag"),
        }
        assert_eq!(tokens[1], Token::Literal(Cow::Borrowed(" ")));
        match &tokens[2] {
            Token::Tag(tag) => assert_eq!(tag.name, "result"),
            _ => panic!("expected Tag"),
        }
    }

    #[test]
    fn test_parse_param_empty_value_after_colon() {
        let (key, value) = parse_param("key:").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "");
    }
}
