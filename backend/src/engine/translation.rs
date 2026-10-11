//! Declarative field paths and arithmetic message translation.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};

use super::{finite_number, integer, integer_value};

pub(super) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

pub(super) fn get_path<'a>(record: &'a Value, path: &str) -> Result<&'a Value> {
    let path = path.strip_prefix("source.").unwrap_or(path);
    if path == "source" || path.is_empty() {
        return Ok(record);
    }
    ensure!(valid_path(path), "Invalid field path {path}");
    let mut value = record;
    for key in path.split('.') {
        value = value
            .get(key)
            .with_context(|| format!("Source field {path} is missing"))?;
    }
    Ok(value)
}

pub(super) fn set_path(record: &mut Value, path: &str, value: Value) -> Result<()> {
    ensure!(valid_path(path), "Invalid destination field {path}");
    let mut current = record;
    let mut keys = path.split('.').peekable();
    while let Some(key) = keys.next() {
        let object = current
            .as_object_mut()
            .with_context(|| format!("Destination {path} overlaps a scalar field"))?;
        if keys.peek().is_none() {
            object.insert(key.into(), value);
            return Ok(());
        }
        current = object.entry(key).or_insert_with(|| json!({}));
    }
    Ok(())
}

pub(super) fn remove_path(record: &mut Value, path: &str) {
    if let Some((parent, key)) = path.rsplit_once('.') {
        let mut current = record;
        for part in parent.split('.') {
            match current.get_mut(part) {
                Some(value) => current = value,
                None => return,
            }
        }
        if let Some(object) = current.as_object_mut() {
            object.remove(key);
        }
    } else if let Some(object) = record.as_object_mut() {
        object.remove(path);
    }
}

#[derive(Debug, Clone)]
pub(super) enum Token {
    Value(Value),
    Field(String),
    Op(String),
    Left,
    Right,
}

pub(super) fn tokens(expression: &str) -> Result<Vec<Token>> {
    ensure!(
        expression.len() <= 2000,
        "Translation expression exceeds 2000 characters"
    );
    let bytes = expression.as_bytes();
    let mut index = 0;
    let mut result = Vec::new();
    while index < bytes.len() {
        let start = index;
        let ch = bytes[index];
        if ch.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        match ch {
            b'(' => { result.push(Token::Left); index += 1; },
            b')' => { result.push(Token::Right); index += 1; },
            b'+' | b'-' | b'*' | b'/' | b'%' => {
                index += 1;
                if ch == b'/' && bytes.get(index) == Some(&b'/') { index += 1; }
                result.push(Token::Op(expression[start..index].into()));
            },
            b'"' => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' { index += 2; continue; }
                    if bytes[index] == b'"' { index += 1; break; }
                    index += 1;
                }
                ensure!(index <= bytes.len(), "Invalid string literal");
                result.push(Token::Value(serde_json::from_str(&expression[start..index])?));
            },
            c if c.is_ascii_digit() || c == b'.' => {
                index += 1;
                while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') { index += 1; }
                if matches!(bytes.get(index), Some(b'e' | b'E')) {
                    index += 1;
                    if matches!(bytes.get(index), Some(b'+' | b'-')) { index += 1; }
                    while index < bytes.len() && bytes[index].is_ascii_digit() { index += 1; }
                }
                let value: Value = serde_json::from_str(&expression[start..index]).context("Invalid numeric literal")?;
                ensure!(value.is_number(), "Expected numeric literal");
                result.push(Token::Value(value));
            },
            c if c.is_ascii_alphabetic() || c == b'_' => {
                index += 1;
                while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' || bytes[index] == b'.') { index += 1; }
                let path = &expression[start..index];
                match path { "true" => result.push(Token::Value(json!(true))), "false" => result.push(Token::Value(json!(false))), "null" => result.push(Token::Value(Value::Null)), _ => {
                    ensure!(valid_path(path), "Invalid source field {path}");
                    result.push(Token::Field(path.into()));
                }}
            },
            _ => bail!("Unsupported expression syntax; use field paths, numbers, JSON strings, parentheses and + - * / // %"),
        }
        ensure!(result.len() <= 200, "Translation expression is too complex");
    }
    Ok(result)
}

pub(super) fn arithmetic(left: Value, op: &str, right: Value) -> Result<Value> {
    if let (Some(a), Some(b)) = (integer(&left), integer(&right)) {
        let result = match op {
            "+" => a.checked_add(b),
            "-" => a.checked_sub(b),
            "*" => a.checked_mul(b),
            "//" if b != 0 => {
                let q = a / b;
                Some(if a % b != 0 && ((a < 0) != (b < 0)) {
                    q - 1
                } else {
                    q
                })
            }
            "%" if b != 0 => Some((a % b + b) % b),
            "/" => None,
            _ => bail!("Invalid arithmetic operation or division by zero"),
        };
        if op != "/" {
            return integer_value(result.context("Integer arithmetic overflow")?);
        }
    }
    let a = left
        .as_f64()
        .context("Arithmetic requires numeric fields")?;
    let b = right
        .as_f64()
        .context("Arithmetic requires numeric fields")?;
    if matches!(op, "/" | "//" | "%") {
        ensure!(b != 0.0, "Division by zero");
    }
    finite_number(match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" => a / b,
        "//" => (a / b).floor(),
        "%" => a - (a / b).floor() * b,
        _ => bail!("Unknown operator {op}"),
    })
}

pub(super) fn evaluate(expression: &str, source: &Value) -> Result<Value> {
    fn parse(tokens: &[Token], index: &mut usize, minimum: u8, source: &Value) -> Result<Value> {
        let token = tokens
            .get(*index)
            .context("Incomplete translation expression")?;
        *index += 1;
        let mut left = match token {
            Token::Value(value) => value.clone(),
            Token::Field(path) => get_path(source, path)?.clone(),
            Token::Left => {
                let value = parse(tokens, index, 0, source)?;
                ensure!(
                    matches!(tokens.get(*index), Some(Token::Right)),
                    "Missing closing parenthesis"
                );
                *index += 1;
                value
            }
            Token::Op(op) if op == "+" || op == "-" => {
                let value = parse(tokens, index, 30, source)?;
                if op == "-" {
                    arithmetic(json!(0), "-", value)?
                } else {
                    ensure!(value.is_number(), "Unary + requires a number");
                    value
                }
            }
            _ => bail!("Expected a field, value or parenthesized expression"),
        };
        while let Some(Token::Op(op)) = tokens.get(*index) {
            let precedence = if op == "+" || op == "-" { 10 } else { 20 };
            if precedence < minimum {
                break;
            }
            *index += 1;
            let right = parse(tokens, index, precedence + 1, source)?;
            left = arithmetic(left, op, right)?;
        }
        Ok(left)
    }
    let tokens = tokens(expression)?;
    let mut index = 0;
    let value = parse(&tokens, &mut index, 0, source)?;
    ensure!(index == tokens.len(), "Unexpected syntax after expression");
    Ok(value)
}

/// Apply safe field mappings. Wildcards copy unmapped fields; explicit renames
/// remove their original source paths, matching the legacy translation scripts.
pub fn translate_record(source: &Value, script: &str) -> Result<Value> {
    if script.trim().is_empty() {
        return Ok(source.clone());
    }
    let mut copies = Vec::new();
    let mut mappings: Vec<(Option<String>, String, String)> = Vec::new();
    for (line_no, raw) in script.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((destination, expression)) = line.split_once("<-") {
            ensure!(
                valid_path(destination.trim()),
                "Line {}: invalid destination",
                line_no + 1
            );
            tokens(expression.trim()).with_context(|| format!("Line {}", line_no + 1))?;
            mappings.push((None, destination.trim().into(), expression.trim().into()));
        } else if let Some((field, destination)) = line.split_once("->") {
            let field = field.trim().strip_prefix("source.").unwrap_or(field.trim());
            let destination = destination.trim();
            ensure!(
                field == "*" || valid_path(field),
                "Line {}: invalid source field",
                line_no + 1
            );
            if destination == "*" {
                copies.push(field.to_owned());
                continue;
            }
            ensure!(
                field != "*",
                "Line {}: use * -> * for a whole record",
                line_no + 1
            );
            let path_end = destination
                .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '.')
                .unwrap_or(destination.len());
            let target = &destination[..path_end];
            ensure!(
                valid_path(target),
                "Line {}: invalid destination",
                line_no + 1
            );
            let expression = format!("source.{field}{}", &destination[path_end..]);
            tokens(&expression).with_context(|| format!("Line {}", line_no + 1))?;
            mappings.push((Some(field.into()), target.into(), expression));
        } else {
            bail!("Line {}: use source.field -> destination or destination <- source.field arithmetic", line_no + 1);
        }
    }
    ensure!(
        !copies.is_empty() || !mappings.is_empty(),
        "Translation contains no mappings"
    );
    let mut output = json!({});
    for copy in copies {
        if copy == "*" {
            output = source.clone();
            ensure!(
                output.is_object(),
                "Wildcard copying requires an object record"
            );
        } else {
            set_path(&mut output, &copy, get_path(source, &copy)?.clone())?;
        }
    }
    for (field, _, _) in &mappings {
        if let Some(field) = field {
            remove_path(&mut output, field);
        }
    }
    for (_, destination, expression) in mappings {
        set_path(&mut output, &destination, evaluate(&expression, source)?)?;
    }
    Ok(output)
}
