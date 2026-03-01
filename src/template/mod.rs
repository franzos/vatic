pub mod functions;
pub mod parser;
pub mod pipes;

use std::future::Future;
use std::pin::Pin;

use crate::error::{Error, Result};

use self::functions::{resolve_tag, LoopValue, RenderContext};
use self::parser::{tokenize, Iterable, Token};
use self::pipes::apply_pipe;

/// Render a template string by tokenizing and resolving tags against the context.
pub async fn render(template: &str, ctx: &RenderContext) -> Result<String> {
    let tokens = tokenize(template)?;
    render_tokens(&tokens, ctx).await
}

fn render_tokens<'a>(
    tokens: &'a [Token<'a>],
    ctx: &'a RenderContext,
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let mut output = String::new();
        let mut i = 0;

        while i < tokens.len() {
            match &tokens[i] {
                Token::Literal(s) => {
                    output.push_str(s);
                    i += 1;
                }
                Token::Tag(tag) => {
                    let value = resolve_tag(tag, ctx)?;
                    let final_value = if let Some(pipe) = &tag.pipe {
                        apply_pipe(pipe, &value).await?
                    } else {
                        value
                    };
                    output.push_str(&final_value);
                    i += 1;
                }
                Token::ForStart(for_loop) => {
                    let (body_tokens, end_idx) = collect_for_body(&tokens[i + 1..])?;
                    let body_output = execute_for_loop(for_loop, &body_tokens, ctx).await?;
                    output.push_str(&body_output);
                    i += 1 + end_idx + 1; // skip past ForEnd
                }
                Token::ForEnd => {
                    return Err(Error::Template("unexpected endfor outside for loop".into()));
                }
            }
        }

        Ok(output)
    })
}

/// Collect body tokens between ForStart and matching ForEnd, tracking nesting depth.
/// Returns owned tokens so they can be passed into recursive render calls.
fn collect_for_body(tokens: &[Token<'_>]) -> Result<(Vec<Token<'static>>, usize)> {
    let mut depth = 0;
    let mut body = Vec::new();

    for (idx, token) in tokens.iter().enumerate() {
        match token {
            Token::ForEnd if depth == 0 => {
                return Ok((body, idx));
            }
            Token::ForEnd => {
                depth -= 1;
                body.push(token.clone().into_owned());
            }
            Token::ForStart(_) => {
                depth += 1;
                body.push(token.clone().into_owned());
            }
            _ => {
                body.push(token.clone().into_owned());
            }
        }
    }

    Err(Error::Template("for loop without matching endfor".into()))
}

/// Execute a for loop — clones the context once and swaps the loop var each iteration.
async fn execute_for_loop(
    for_loop: &parser::ForLoop,
    body: &[Token<'static>],
    ctx: &RenderContext,
) -> Result<String> {
    let mut output = String::new();
    let mut iter_ctx = ctx.clone();

    match &for_loop.iterable {
        Iterable::Range(start, end) => {
            for val in *start..=*end {
                iter_ctx
                    .loop_vars
                    .insert(for_loop.var.clone(), LoopValue::Index(val));
                let rendered = render_tokens(body, &iter_ctx).await?;
                output.push_str(&rendered);
            }
        }
        Iterable::Collection(name) => {
            let items = get_collection(name, ctx)?;
            let limit = for_loop
                .params
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok());

            let take_count = match limit {
                Some(lim) => items.len().min(lim),
                None => items.len(),
            };

            for item in items.into_iter().take(take_count) {
                iter_ctx.loop_vars.insert(for_loop.var.clone(), item);
                let rendered = render_tokens(body, &iter_ctx).await?;
                output.push_str(&rendered);
            }
        }
    }

    Ok(output)
}

/// Resolve a named collection from the context.
fn get_collection(name: &str, ctx: &RenderContext) -> Result<Vec<LoopValue>> {
    match name {
        "memories" => Ok(ctx
            .memories
            .iter()
            .map(|m| LoopValue::Memory(m.clone()))
            .collect()),
        _ => Err(Error::Template(format!("unknown collection: '{name}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dictionary::Dictionary;
    use crate::template::functions::{MemoryEntry, RenderContext};

    fn ctx_with_dict() -> RenderContext {
        let mut dict = Dictionary::new();
        dict.entries
            .entry("general".into())
            .or_default()
            .insert("name".into(), "Franz".into());
        RenderContext::new(dict)
    }

    #[tokio::test]
    async fn test_full_template() {
        let ctx = ctx_with_dict();
        let result = render("Hello {% custom:name %}, today is {% date %}", &ctx)
            .await
            .unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(result, format!("Hello Franz, today is {today}"));
    }

    #[tokio::test]
    async fn test_missing_result() {
        let ctx = ctx_with_dict();
        let result = render("Result: {% result %}", &ctx).await.unwrap();
        assert_eq!(result, "Result: ");
    }

    #[tokio::test]
    async fn test_for_range_render() {
        let ctx = ctx_with_dict();
        let template = "{% for i in (1..3) %}item {% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "item item item ");
    }

    #[tokio::test]
    async fn test_for_memories_render() {
        let mut ctx = ctx_with_dict();
        ctx.memories = vec![
            MemoryEntry {
                date: "2025-01-01".into(),
                datetime: "2025-01-01 08:00".into(),
                result: "sunny".into(),
            },
            MemoryEntry {
                date: "2025-01-02".into(),
                datetime: "2025-01-02 08:00".into(),
                result: "rainy".into(),
            },
        ];
        let template = "{% for i in memories limit:3 %}Date: {% i.date %}\n{% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "Date: 2025-01-01\nDate: 2025-01-02\n");
    }

    #[tokio::test]
    async fn test_for_memories_with_limit() {
        let mut ctx = ctx_with_dict();
        ctx.memories = vec![
            MemoryEntry {
                date: "2025-01-01".into(),
                datetime: "2025-01-01 08:00".into(),
                result: "a".into(),
            },
            MemoryEntry {
                date: "2025-01-02".into(),
                datetime: "2025-01-02 08:00".into(),
                result: "b".into(),
            },
            MemoryEntry {
                date: "2025-01-03".into(),
                datetime: "2025-01-03 08:00".into(),
                result: "c".into(),
            },
        ];
        let template = "{% for i in memories limit:2 %}{% i.result %} {% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "a b ");
    }

    #[tokio::test]
    async fn test_literal_only() {
        let ctx = ctx_with_dict();
        let result = render("just plain text", &ctx).await.unwrap();
        assert_eq!(result, "just plain text");
    }

    #[tokio::test]
    async fn test_unclosed_for_loop_error() {
        let ctx = ctx_with_dict();
        let err = render("{% for i in (1..3) %}hello", &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("without matching endfor"));
    }

    #[tokio::test]
    async fn test_unexpected_endfor_error() {
        let ctx = ctx_with_dict();
        let err = render("{% endfor %}", &ctx).await.unwrap_err();
        assert!(err.to_string().contains("unexpected endfor"));
    }

    #[tokio::test]
    async fn test_nested_for_loop_render() {
        let ctx = ctx_with_dict();
        let template =
            "{% for i in (1..2) %}{% for j in (1..2) %}({% i %},{% j %}) {% endfor %}{% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "(1,1) (1,2) (2,1) (2,2) ");
    }

    #[tokio::test]
    async fn test_for_range_same_start_end() {
        let ctx = ctx_with_dict();
        let template = "{% for i in (3..3) %}{% i %} {% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "3 ");
    }

    #[tokio::test]
    async fn test_unknown_collection_error() {
        let ctx = ctx_with_dict();
        let err = render("{% for i in foobar %}{% endfor %}", &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown collection: 'foobar'"));
    }

    #[tokio::test]
    async fn test_empty_memories_collection() {
        let ctx = ctx_with_dict();
        let template = "{% for i in memories %}{% i.result %} {% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_for_memories_limit_larger_than_available() {
        let mut ctx = ctx_with_dict();
        ctx.memories = vec![
            MemoryEntry {
                date: "2025-01-01".into(),
                datetime: "2025-01-01 08:00".into(),
                result: "first".into(),
            },
            MemoryEntry {
                date: "2025-01-02".into(),
                datetime: "2025-01-02 08:00".into(),
                result: "second".into(),
            },
        ];
        let template = "{% for i in memories limit:10 %}{% i.result %} {% endfor %}";
        let result = render(template, &ctx).await.unwrap();
        assert_eq!(result, "first second ");
    }
}
