use std::collections::HashMap;

use chrono::{Duration, Local};

use crate::config::dictionary::Dictionary;
use crate::config::secrets::Secrets;
use crate::error::{Error, Result};
use crate::template::parser::TagContent;

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub date: String,
    pub datetime: String,
    pub result: String,
}

#[derive(Debug, Clone)]
pub enum LoopValue {
    Index(i64),
    Memory(MemoryEntry),
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub dictionary: Dictionary,
    pub secrets: Secrets,
    pub result: Option<String>,
    pub message: Option<String>,
    pub sender: Option<String>,
    pub memories: Vec<MemoryEntry>,
    pub loop_vars: HashMap<String, LoopValue>,
}

impl RenderContext {
    /// Minimal context — just a dictionary, no memories or result yet.
    pub fn new(dictionary: Dictionary) -> Self {
        Self {
            dictionary,
            secrets: Secrets::default(),
            result: None,
            message: None,
            sender: None,
            memories: vec![],
            loop_vars: HashMap::new(),
        }
    }
}

/// Resolve a template tag to its string value.
pub fn resolve_tag(tag: &TagContent, ctx: &RenderContext) -> Result<String> {
    let name = &tag.name;

    // Dotted access for loop variables: `i.date`, `i.result`, etc.
    if let Some(dot_pos) = name.find('.') {
        let var_name = &name[..dot_pos];
        let field = &name[dot_pos + 1..];
        return resolve_loop_var_field(var_name, field, ctx);
    }

    // proxy:name — resolves to the match URL from secrets
    if let Some(secret_name) = name.strip_prefix("proxy:") {
        return resolve_proxy(secret_name, ctx);
    }

    // custom:key — dictionary lookup
    if let Some(key) = name.strip_prefix("custom:") {
        return resolve_custom(key, ctx);
    }

    match name.as_str() {
        "date" => resolve_date(tag, ctx),
        "datetime" => resolve_datetime(tag, ctx),
        "datetimeiso" => resolve_datetimeiso(tag),
        "result" => Ok(ctx.result.clone().unwrap_or_default()),
        "message" => Ok(ctx.message.clone().unwrap_or_default()),
        "sender" => Ok(ctx.sender.clone().unwrap_or_default()),
        "memory" => resolve_memory(tag, ctx),
        _ => {
            // Fall through to loop variables
            if let Some(loop_val) = ctx.loop_vars.get(name.as_str()) {
                match loop_val {
                    LoopValue::Index(i) => Ok(i.to_string()),
                    LoopValue::Memory(m) => Ok(m.result.clone()),
                }
            } else {
                Err(Error::Template(format!("unknown tag: '{name}'")))
            }
        }
    }
}

fn resolve_proxy(name: &str, ctx: &RenderContext) -> Result<String> {
    ctx.secrets
        .get(name)
        .map(|s| s.match_url.clone())
        .ok_or_else(|| Error::Template(format!("unknown secret for proxy: '{name}'")))
}

fn resolve_custom(key: &str, ctx: &RenderContext) -> Result<String> {
    ctx.dictionary
        .get("general", key)
        .map(|s| s.to_string())
        .ok_or_else(|| Error::Template(format!("unknown dictionary key: 'custom:{key}'")))
}

fn resolve_date(tag: &TagContent, ctx: &RenderContext) -> Result<String> {
    let now = Local::now();
    let offset = compute_offset(&tag.params, ctx)?;
    let dt = now + offset;
    Ok(dt.format("%Y-%m-%d").to_string())
}

fn resolve_datetime(tag: &TagContent, ctx: &RenderContext) -> Result<String> {
    let now = Local::now();
    let offset = compute_offset(&tag.params, ctx)?;
    let dt = now + offset;
    Ok(dt.format("%Y-%m-%d %H:%M").to_string())
}

fn resolve_datetimeiso(_tag: &TagContent) -> Result<String> {
    let now = Local::now();
    Ok(now.to_rfc3339())
}

fn resolve_memory(tag: &TagContent, ctx: &RenderContext) -> Result<String> {
    // minus=1 is the default (latest), minus=2 is second latest, etc.
    let offset = if let Some(minus_str) = tag.params.get("minus") {
        let val: usize = minus_str
            .parse()
            .map_err(|_| Error::Template(format!("invalid memory offset: '{minus_str}'")))?;
        if val == 0 {
            0
        } else {
            val - 1
        }
    } else {
        0
    };

    ctx.memories
        .get(offset)
        .map(|m| m.result.clone())
        .ok_or_else(|| {
            Error::Template(format!(
                "no memory at offset {offset} (have {} memories)",
                ctx.memories.len()
            ))
        })
}

fn resolve_loop_var_field(var_name: &str, field: &str, ctx: &RenderContext) -> Result<String> {
    let loop_val = ctx
        .loop_vars
        .get(var_name)
        .ok_or_else(|| Error::Template(format!("unknown loop variable: '{var_name}'")))?;

    match loop_val {
        LoopValue::Index(_) => Err(Error::Template(format!(
            "index variable '{var_name}' has no field '{field}'"
        ))),
        LoopValue::Memory(m) => match field {
            "date" => Ok(m.date.clone()),
            "datetime" => Ok(m.datetime.clone()),
            "result" => Ok(m.result.clone()),
            _ => Err(Error::Template(format!("memory has no field '{field}'"))),
        },
    }
}

/// Compute a time offset from `minus` and `plus` params.
/// Supports loop variable interpolation like `minus=i"d"` where `i` is an index.
fn compute_offset(params: &HashMap<String, String>, ctx: &RenderContext) -> Result<Duration> {
    let mut total = Duration::zero();

    if let Some(minus_val) = params.get("minus") {
        let resolved = resolve_param_value(minus_val, ctx)?;
        let dur = parse_duration(&resolved)?;
        total -= dur;
    }

    if let Some(plus_val) = params.get("plus") {
        let resolved = resolve_param_value(plus_val, ctx)?;
        let dur = parse_duration(&resolved)?;
        total += dur;
    }

    Ok(total)
}

/// Resolve a param value, handling variable interpolation.
/// Pattern: `i"d"` — loop var `i` gets its index value prepended to the suffix.
fn resolve_param_value(value: &str, ctx: &RenderContext) -> Result<String> {
    // A quote means we've got variable interpolation
    if let Some(quote_pos) = value.find('"') {
        let var_part = &value[..quote_pos];
        let rest = &value[quote_pos + 1..];

        // Strip trailing quote
        let suffix = rest.trim_end_matches('"');

        if let Some(loop_val) = ctx.loop_vars.get(var_part) {
            match loop_val {
                LoopValue::Index(i) => {
                    return Ok(format!("{i}{suffix}"));
                }
                _ => {
                    return Err(Error::Template(format!(
                        "loop variable '{var_part}' is not an index, cannot interpolate"
                    )));
                }
            }
        }
    }

    Ok(value.to_string())
}

/// Parse a duration string like `1d`, `2h`, `30m`.
fn parse_duration(input: &str) -> Result<Duration> {
    if input.is_empty() {
        return Err(Error::Template("empty duration".into()));
    }

    let (num_str, unit) = input.split_at(input.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| Error::Template(format!("invalid duration number: '{num_str}'")))?;

    match unit {
        "d" => Ok(Duration::days(num)),
        "h" => Ok(Duration::hours(num)),
        "m" => Ok(Duration::minutes(num)),
        _ => Err(Error::Template(format!("unknown duration unit: '{unit}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ctx() -> RenderContext {
        RenderContext::new(Dictionary::new())
    }

    fn tag(name: &str) -> TagContent {
        TagContent {
            name: name.to_string(),
            params: HashMap::new(),
            pipe: None,
        }
    }

    fn tag_with_params(name: &str, params: Vec<(&str, &str)>) -> TagContent {
        TagContent {
            name: name.to_string(),
            params: params
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            pipe: None,
        }
    }

    #[test]
    fn test_date_today() {
        let ctx = empty_ctx();
        let result = resolve_tag(&tag("date"), &ctx).unwrap();
        let expected = Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_date_minus() {
        let ctx = empty_ctx();
        let t = tag_with_params("date", vec![("minus", "1d")]);
        let result = resolve_tag(&t, &ctx).unwrap();
        let expected = (Local::now() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_datetime_plus() {
        let ctx = empty_ctx();
        let t = tag_with_params("datetime", vec![("plus", "1h")]);
        let result = resolve_tag(&t, &ctx).unwrap();
        let expected = (Local::now() + Duration::hours(1))
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_custom_key() {
        let mut dict = Dictionary::new();
        dict.entries
            .entry("general".into())
            .or_default()
            .insert("name".into(), "Franz".into());

        let ctx = RenderContext::new(dict);
        let result = resolve_tag(&tag("custom:name"), &ctx).unwrap();
        assert_eq!(result, "Franz");
    }

    #[test]
    fn test_custom_unknown() {
        let ctx = empty_ctx();
        let result = resolve_tag(&tag("custom:unknown"), &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_result_substitution() {
        let mut ctx = empty_ctx();
        ctx.result = Some("sunny and warm".into());
        let result = resolve_tag(&tag("result"), &ctx).unwrap();
        assert_eq!(result, "sunny and warm");
    }

    #[test]
    fn test_message_substitution() {
        let mut ctx = empty_ctx();
        ctx.message = Some("what's the weather?".into());
        let result = resolve_tag(&tag("message"), &ctx).unwrap();
        assert_eq!(result, "what's the weather?");
    }

    #[test]
    fn test_result_missing_returns_empty() {
        let ctx = empty_ctx();
        let result = resolve_tag(&tag("result"), &ctx).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("1d").unwrap(), Duration::days(1));
        assert_eq!(parse_duration("3h").unwrap(), Duration::hours(3));
        assert_eq!(parse_duration("30m").unwrap(), Duration::minutes(30));
    }

    #[test]
    fn test_resolve_param_value_with_loop_var() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert("i".into(), LoopValue::Index(2));
        let result = resolve_param_value("i\"d\"", &ctx).unwrap();
        assert_eq!(result, "2d");
    }

    #[test]
    fn test_proxy_substitution() {
        let mut ctx = empty_ctx();
        ctx.secrets.entries.insert(
            "formshive".into(),
            crate::config::secrets::Secret {
                key: "abc123".into(),
                header: "bearer".into(),
                match_url: "https://api.formshive.com".into(),
            },
        );
        let result = resolve_tag(&tag("proxy:formshive"), &ctx).unwrap();
        assert_eq!(result, "https://api.formshive.com");
    }

    #[test]
    fn test_proxy_unknown() {
        let ctx = empty_ctx();
        let result = resolve_tag(&tag("proxy:unknown"), &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_default() {
        let mut ctx = empty_ctx();
        ctx.memories.push(MemoryEntry {
            date: "2025-01-01".into(),
            datetime: "2025-01-01 08:00".into(),
            result: "sunny".into(),
        });
        let t = tag("memory");
        let result = resolve_tag(&t, &ctx).unwrap();
        assert_eq!(result, "sunny");
    }

    #[test]
    fn test_memory_minus_1() {
        let mut ctx = empty_ctx();
        ctx.memories.push(MemoryEntry {
            date: "d".into(),
            datetime: "dt".into(),
            result: "first".into(),
        });
        ctx.memories.push(MemoryEntry {
            date: "d".into(),
            datetime: "dt".into(),
            result: "second".into(),
        });
        let t = tag_with_params("memory", vec![("minus", "1")]);
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "first");
    }

    #[test]
    fn test_memory_minus_2() {
        let mut ctx = empty_ctx();
        ctx.memories.push(MemoryEntry {
            date: "d".into(),
            datetime: "dt".into(),
            result: "first".into(),
        });
        ctx.memories.push(MemoryEntry {
            date: "d".into(),
            datetime: "dt".into(),
            result: "second".into(),
        });
        let t = tag_with_params("memory", vec![("minus", "2")]);
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "second");
    }

    #[test]
    fn test_memory_minus_0() {
        let mut ctx = empty_ctx();
        ctx.memories.push(MemoryEntry {
            date: "d".into(),
            datetime: "dt".into(),
            result: "only".into(),
        });
        let t = tag_with_params("memory", vec![("minus", "0")]);
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "only");
    }

    #[test]
    fn test_memory_empty() {
        let ctx = empty_ctx();
        let t = tag("memory");
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("no memory at offset"));
    }

    #[test]
    fn test_memory_invalid_offset() {
        let ctx = empty_ctx();
        let t = tag_with_params("memory", vec![("minus", "abc")]);
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("invalid memory offset"));
    }

    #[test]
    fn test_loop_var_memory_date() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert(
            "i".into(),
            LoopValue::Memory(MemoryEntry {
                date: "2025-01-01".into(),
                datetime: "2025-01-01 08:00".into(),
                result: "sunny".into(),
            }),
        );
        let t = tag("i.date");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "2025-01-01");
    }

    #[test]
    fn test_loop_var_memory_result() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert(
            "i".into(),
            LoopValue::Memory(MemoryEntry {
                date: "d".into(),
                datetime: "dt".into(),
                result: "cloudy".into(),
            }),
        );
        let t = tag("i.result");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "cloudy");
    }

    #[test]
    fn test_loop_var_memory_datetime() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert(
            "i".into(),
            LoopValue::Memory(MemoryEntry {
                date: "d".into(),
                datetime: "2025-01-01 08:00".into(),
                result: "r".into(),
            }),
        );
        let t = tag("i.datetime");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "2025-01-01 08:00");
    }

    #[test]
    fn test_loop_var_memory_unknown_field() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert(
            "i".into(),
            LoopValue::Memory(MemoryEntry {
                date: "d".into(),
                datetime: "dt".into(),
                result: "r".into(),
            }),
        );
        let t = tag("i.bogus");
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("no field 'bogus'"));
    }

    #[test]
    fn test_loop_var_index_field_error() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert("i".into(), LoopValue::Index(5));
        let t = tag("i.date");
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("no field 'date'"));
    }

    #[test]
    fn test_loop_var_unknown_var() {
        let ctx = empty_ctx();
        let t = tag("x.date");
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("unknown loop variable"));
    }

    #[test]
    fn test_datetimeiso_format() {
        let ctx = empty_ctx();
        let t = tag("datetimeiso");
        let result = resolve_tag(&t, &ctx).unwrap();
        // Should be RFC3339 format, e.g. "2025-01-15T14:30:00+01:00"
        assert!(
            result.contains("T"),
            "expected RFC3339 format with 'T' separator"
        );
        assert!(result.len() > 20, "expected full RFC3339 timestamp");
    }

    #[test]
    fn test_parse_duration_zero() {
        assert_eq!(parse_duration("0d").unwrap(), Duration::zero());
        assert_eq!(parse_duration("0h").unwrap(), Duration::zero());
        assert_eq!(parse_duration("0m").unwrap(), Duration::zero());
    }

    #[test]
    fn test_parse_duration_large() {
        assert_eq!(parse_duration("365d").unwrap(), Duration::days(365));
    }

    #[test]
    fn test_parse_duration_empty() {
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn test_parse_duration_unknown_unit() {
        let err = parse_duration("1x").unwrap_err();
        assert!(err.to_string().contains("unknown duration unit"));
    }

    #[test]
    fn test_parse_duration_no_number() {
        let err = parse_duration("d").unwrap_err();
        assert!(err.to_string().contains("invalid duration number"));
    }

    #[test]
    fn test_parse_duration_negative() {
        assert_eq!(parse_duration("-1d").unwrap(), Duration::days(-1));
    }

    #[test]
    fn test_compute_offset_both() {
        let ctx = empty_ctx();
        let mut params = HashMap::new();
        params.insert("minus".into(), "2d".into());
        params.insert("plus".into(), "1d".into());
        let offset = compute_offset(&params, &ctx).unwrap();
        assert_eq!(offset, Duration::days(-1));
    }

    #[test]
    fn test_compute_offset_none() {
        let ctx = empty_ctx();
        let params = HashMap::new();
        let offset = compute_offset(&params, &ctx).unwrap();
        assert_eq!(offset, Duration::zero());
    }

    #[test]
    fn test_resolve_param_no_interpolation() {
        let ctx = empty_ctx();
        let result = resolve_param_value("1d", &ctx).unwrap();
        assert_eq!(result, "1d");
    }

    #[test]
    fn test_resolve_param_memory_var_error() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert(
            "i".into(),
            LoopValue::Memory(MemoryEntry {
                date: "d".into(),
                datetime: "dt".into(),
                result: "r".into(),
            }),
        );
        let err = resolve_param_value("i\"d\"", &ctx).unwrap_err();
        assert!(err.to_string().contains("not an index"));
    }

    #[test]
    fn test_bare_loop_var_index() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert("i".into(), LoopValue::Index(42));
        let t = tag("i");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "42");
    }

    #[test]
    fn test_bare_loop_var_memory() {
        let mut ctx = empty_ctx();
        ctx.loop_vars.insert(
            "m".into(),
            LoopValue::Memory(MemoryEntry {
                date: "d".into(),
                datetime: "dt".into(),
                result: "the result".into(),
            }),
        );
        let t = tag("m");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "the result");
    }

    #[test]
    fn test_unknown_tag() {
        let ctx = empty_ctx();
        let t = tag("nonexistent");
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("unknown tag"));
    }

    #[test]
    fn test_sender_substitution() {
        let mut ctx = empty_ctx();
        ctx.sender = Some("alice".into());
        let t = tag("sender");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "alice");
    }

    #[test]
    fn test_sender_missing_returns_empty() {
        let ctx = empty_ctx();
        let t = tag("sender");
        assert_eq!(resolve_tag(&t, &ctx).unwrap(), "");
    }

    #[test]
    fn test_parse_duration_decimal() {
        let err = parse_duration("1.5d").unwrap_err();
        assert!(err.to_string().contains("invalid duration number"));
    }

    #[test]
    fn test_parse_duration_with_whitespace() {
        let err = parse_duration(" 1d").unwrap_err();
        assert!(err.to_string().contains("invalid duration number"));
    }

    #[test]
    fn test_memory_offset_beyond_available() {
        let mut ctx = empty_ctx();
        ctx.memories.push(MemoryEntry {
            date: "2025-01-01".into(),
            datetime: "2025-01-01 08:00".into(),
            result: "only".into(),
        });
        let t = tag_with_params("memory", vec![("minus", "10")]);
        let err = resolve_tag(&t, &ctx).unwrap_err();
        assert!(err.to_string().contains("no memory at offset"));
    }

    #[test]
    fn test_resolve_param_value_unknown_var_in_interpolation() {
        let ctx = empty_ctx();
        let result = resolve_param_value("x\"d\"", &ctx).unwrap();
        assert_eq!(result, "x\"d\"");
    }

    #[test]
    fn test_compute_offset_only_plus() {
        let ctx = empty_ctx();
        let mut params = HashMap::new();
        params.insert("plus".into(), "2h".into());
        let offset = compute_offset(&params, &ctx).unwrap();
        assert_eq!(offset, Duration::hours(2));
    }

    #[test]
    fn test_compute_offset_only_minus() {
        let ctx = empty_ctx();
        let mut params = HashMap::new();
        params.insert("minus".into(), "3d".into());
        let offset = compute_offset(&params, &ctx).unwrap();
        assert_eq!(offset, Duration::days(-3));
    }
}
