use super::interpreter::FeelInterpreter;
use super::value::FeelValue;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::sync::OnceLock;

fn regex_with_flags(pattern: &str, flags: Option<&str>) -> Result<Regex, String> {
    let mut builder = RegexBuilder::new(pattern);
    for flag in flags.unwrap_or_default().chars() {
        match flag {
            'i' => builder.case_insensitive(true),
            'm' => builder.multi_line(true),
            's' => builder.dot_matches_new_line(true),
            'x' => builder.ignore_whitespace(true),
            _ => return Err(format!("unsupported regular-expression flag '{flag}'")),
        };
    }
    builder.build().map_err(|error| error.to_string())
}

fn list_or_varargs<'a>(args: &'a [FeelValue], name: &str) -> Result<&'a [FeelValue], String> {
    if args.is_empty() {
        return Err(format!("{name} requires at least 1 argument"));
    }
    if args.len() == 1
        && let FeelValue::List(items) = &args[0]
    {
        return Ok(items);
    }
    Ok(args)
}

fn numeric_args(args: &[FeelValue], name: &str) -> Result<Vec<f64>, String> {
    list_or_varargs(args, name)?
        .iter()
        .map(|value| {
            value
                .to_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("{name}: all values must be finite numbers"))
        })
        .collect()
}

fn scale_arg(args: &[FeelValue], name: &str) -> Result<(f64, i32), String> {
    if args.len() != 2 {
        return Err(format!("{name}(n, scale) requires 2 arguments"));
    }
    let number = args[0]
        .to_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| format!("{name}: n must be a finite number"))?;
    let scale = args[1]
        .to_f64()
        .filter(|scale| scale.is_finite() && scale.fract() == 0.0)
        .ok_or_else(|| format!("{name}: scale must be an integer"))?;
    if !(-308.0..=308.0).contains(&scale) {
        return Err(format!("{name}: scale is out of range"));
    }
    Ok((number, scale as i32))
}

fn scaled_round(number: f64, scale: i32, operation: impl FnOnce(f64) -> f64) -> FeelValue {
    let factor = 10_f64.powi(scale);
    let result = operation(number * factor) / factor;
    if result.is_finite() {
        FeelValue::Number(result)
    } else {
        FeelValue::Null
    }
}

fn context_put_path(
    context: &mut HashMap<String, FeelValue>,
    keys: &[String],
    value: FeelValue,
) -> Result<(), String> {
    let Some((key, rest)) = keys.split_first() else {
        return Err("context put: key path must not be empty".into());
    };
    if rest.is_empty() {
        context.insert(key.clone(), value);
        return Ok(());
    }
    let entry = context
        .get_mut(key)
        .ok_or_else(|| format!("context put: key '{key}' does not exist"))?;
    let FeelValue::Context(nested) = entry else {
        return Err(format!("context put: key '{key}' is not a context"));
    };
    context_put_path(nested, rest, value)
}

fn parse_date(input: &str, function: &str) -> Result<NaiveDate, String> {
    let date = input.split('T').next().unwrap_or(input);
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|error| format!("{function}: invalid date '{input}': {error}"))
}

fn parse_time(input: &str, function: &str) -> Result<NaiveTime, String> {
    let time = input.split_once('T').map_or(input, |(_, time)| time);
    if let Ok(date_time) = DateTime::parse_from_rfc3339(&format!("1970-01-01T{time}")) {
        return Ok(date_time.time());
    }
    NaiveTime::parse_from_str(time, "%H:%M:%S%.f")
        .map_err(|error| format!("{function}: invalid time '{input}': {error}"))
}

fn validate_date_time(input: &str) -> Result<(), String> {
    if DateTime::parse_from_rfc3339(input).is_ok()
        || NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
    {
        Ok(())
    } else {
        Err(format!("date and time: invalid value '{input}'"))
    }
}

fn validate_duration(input: &str) -> Result<(), String> {
    static DURATION: OnceLock<Regex> = OnceLock::new();
    let regex = DURATION.get_or_init(|| {
        Regex::new(
            r"^-?P(?:(?:(?:\d+)Y)?(?:(?:\d+)M)?|(?:(?:\d+)D)?(?:T(?:(?:\d+)H)?(?:(?:\d+)M)?(?:(?:\d+(?:\.\d+)?)S)?)?)$",
        )
        .expect("duration regex is valid")
    });
    let has_value = input.chars().any(|character| character.is_ascii_digit());
    if has_value && regex.is_match(input) {
        Ok(())
    } else {
        Err(format!("duration: invalid value '{input}'"))
    }
}

fn integer_component(value: &FeelValue, name: &str) -> Result<i64, String> {
    value
        .to_f64()
        .filter(|value| value.is_finite() && value.fract() == 0.0)
        .map(|value| value as i64)
        .ok_or_else(|| format!("{name} must be an integer"))
}

fn format_second(second: f64) -> String {
    if second.fract() == 0.0 {
        format!("{:02}", second as u32)
    } else {
        let formatted = format!("{second:.9}").trim_end_matches('0').to_string();
        if second < 10.0 {
            format!("0{formatted}")
        } else {
            formatted
        }
    }
}

impl FeelInterpreter {
    pub(crate) fn call_function(
        &self,
        name: &str,
        args: &[FeelValue],
        ctx: &HashMap<String, FeelValue>,
    ) -> Result<FeelValue, String> {
        match name.to_lowercase().as_str() {
            // ---- Boolean function (DMN 1.5) ----
            "not" => {
                if args.len() != 1 {
                    return Err("not(x) requires 1 argument".into());
                }
                // Allow null as input for ternary logic; if actual Not AST node, we still return null
                match &args[0] {
                    FeelValue::Bool(b) => Ok(FeelValue::Bool(!*b)),
                    FeelValue::Null => Ok(FeelValue::Null), // ternary: not(null) = null
                    _ => Ok(FeelValue::Null),               // DMN 1.5: invalid arg returns null
                }
            }

            // ---- 字符串函数 ----
            "string" | "string_" => {
                if args.is_empty() {
                    return Err("string() requires 1 argument".into());
                }
                Ok(FeelValue::String(format!("{}", args[0])))
            }
            "substring" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("substring(s, start[, length]) requires at least 2 args".into());
                }
                let s = match &args[0] {
                    FeelValue::String(s) => s.clone(),
                    _ => return Err("substring: first arg must be string".into()),
                };
                let start = args[1]
                    .to_f64()
                    .filter(|value| value.is_finite() && value.fract() == 0.0)
                    .ok_or("substring: start position must be an integer")?;
                let start_pos = start as isize;
                // DMN 1.5: 1-based start position, -1 is last char
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len();
                let start_idx = if start_pos < 0 {
                    len as isize + start_pos
                } else if start_pos == 0 {
                    return Err("substring: start position must not be zero".into());
                } else {
                    start_pos - 1
                };
                if !(0..len as isize).contains(&start_idx) {
                    return Err("substring: start position is out of range".into());
                }
                let start_idx = start_idx as usize;
                if args.len() >= 3 {
                    let sub_len = args[2]
                        .to_f64()
                        .filter(|value| value.is_finite() && value.fract() == 0.0 && *value >= 0.0)
                        .ok_or("substring: length must be a non-negative integer")?
                        as usize;
                    Ok(FeelValue::String(
                        chars[start_idx..].iter().take(sub_len).collect(),
                    ))
                } else {
                    Ok(FeelValue::String(chars[start_idx..].iter().collect()))
                }
            }
            "string length" => {
                if args.len() != 1 {
                    return Err("string length(s) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::String(s) => Ok(FeelValue::Number(s.chars().count() as f64)),
                    _ => Err("string length: argument must be string".into()),
                }
            }
            "upper case" => {
                if args.len() != 1 {
                    return Err("upper case(s) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::String(s) => Ok(FeelValue::String(s.to_uppercase())),
                    _ => Err("upper case: argument must be string".into()),
                }
            }
            "lower case" => {
                if args.len() != 1 {
                    return Err("lower case(s) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::String(s) => Ok(FeelValue::String(s.to_lowercase())),
                    _ => Err("lower case: argument must be string".into()),
                }
            }
            "contains" => {
                if args.len() != 2 {
                    return Err("contains(s, sub) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(s), FeelValue::String(sub)) => {
                        Ok(FeelValue::Bool(s.contains(sub.as_str())))
                    }
                    _ => Err("contains: both arguments must be strings".into()),
                }
            }
            "starts with" => {
                if args.len() != 2 {
                    return Err("starts with(s, prefix) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(s), FeelValue::String(prefix)) => {
                        Ok(FeelValue::Bool(s.starts_with(prefix.as_str())))
                    }
                    _ => Err("starts with: both arguments must be strings".into()),
                }
            }
            "ends with" => {
                if args.len() != 2 {
                    return Err("ends with(s, suffix) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(s), FeelValue::String(suffix)) => {
                        Ok(FeelValue::Bool(s.ends_with(suffix.as_str())))
                    }
                    _ => Err("ends with: both arguments must be strings".into()),
                }
            }
            "replace" => {
                if !(3..=4).contains(&args.len()) {
                    return Err(
                        "replace(input, pattern, replacement[, flags]) requires 3 or 4 arguments"
                            .into(),
                    );
                }
                match (&args[0], &args[1], &args[2]) {
                    (
                        FeelValue::String(input),
                        FeelValue::String(pattern),
                        FeelValue::String(replacement),
                    ) => {
                        let flags = match args.get(3) {
                            Some(FeelValue::String(flags)) => Some(flags.as_str()),
                            Some(_) => return Err("replace: flags must be a string".into()),
                            None => None,
                        };
                        let regex = regex_with_flags(pattern, flags)
                            .map_err(|error| format!("replace: invalid pattern: {error}"))?;
                        Ok(FeelValue::String(
                            regex.replace_all(input, replacement.as_str()).into_owned(),
                        ))
                    }
                    _ => Err("replace: all arguments must be strings".into()),
                }
            }
            "split" => {
                if args.len() != 2 {
                    return Err("split(s, sep) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(s), FeelValue::String(sep)) => {
                        let regex = regex_with_flags(sep, None)
                            .map_err(|error| format!("split: invalid pattern: {error}"))?;
                        let parts: Vec<FeelValue> = regex
                            .split(s)
                            .map(|p| FeelValue::String(p.to_string()))
                            .collect();
                        Ok(FeelValue::List(parts))
                    }
                    _ => Err("split: all arguments must be strings".into()),
                }
            }
            "concatenate" => {
                if args.is_empty() {
                    return Err("concatenate requires at least 1 argument".into());
                }
                let mut result = Vec::new();
                for arg in args {
                    match arg {
                        FeelValue::List(items) => result.extend(items.iter().cloned()),
                        _ => return Err("concatenate: every argument must be a list".into()),
                    }
                }
                Ok(FeelValue::List(result))
            }
            "string join" => {
                if args.is_empty() {
                    return Err("string join requires at least 1 argument".into());
                }
                let sep = if args.len() >= 2 {
                    match &args[1] {
                        FeelValue::String(s) => s.clone(),
                        FeelValue::Null => "".to_string(),
                        _ => "".to_string(),
                    }
                } else {
                    "".to_string()
                };
                match &args[0] {
                    FeelValue::List(items) => {
                        let strs: Vec<String> = items
                            .iter()
                            .filter(|v| !matches!(v, FeelValue::Null))
                            .map(|v| match v {
                                FeelValue::String(s) => s.clone(),
                                other => format!("{}", other),
                            })
                            .collect();
                        Ok(FeelValue::String(strs.join(&sep)))
                    }
                    _ => Err("string join: first argument must be a list".into()),
                }
            }
            "substring before" => {
                if args.len() != 2 {
                    return Err("substring before(s, match) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(s), FeelValue::String(m)) => match s.find(m.as_str()) {
                        Some(pos) => Ok(FeelValue::String(s[..pos].to_string())),
                        None => Ok(FeelValue::String("".to_string())),
                    },
                    _ => Err("substring before: both arguments must be strings".into()),
                }
            }
            "substring after" => {
                if args.len() != 2 {
                    return Err("substring after(s, match) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(s), FeelValue::String(m)) => match s.find(m.as_str()) {
                        Some(pos) => Ok(FeelValue::String(s[pos + m.len()..].to_string())),
                        None => Ok(FeelValue::String("".to_string())),
                    },
                    _ => Err("substring after: both arguments must be strings".into()),
                }
            }
            "matches" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("matches(input, pattern[, flags]) requires 2 or 3 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::String(input), FeelValue::String(pattern)) => {
                        let flags = match args.get(2) {
                            Some(FeelValue::String(flags)) => Some(flags.as_str()),
                            Some(_) => return Err("matches: flags must be a string".into()),
                            None => None,
                        };
                        let regex = regex_with_flags(pattern, flags)
                            .map_err(|error| format!("matches: invalid pattern: {error}"))?;
                        Ok(FeelValue::Bool(regex.is_match(input)))
                    }
                    _ => Err("matches: both arguments must be strings".into()),
                }
            }

            // ---- 列表函数 ----
            "count" => {
                if args.len() != 1 {
                    return Err("count(list) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::List(items) => Ok(FeelValue::Number(items.len() as f64)),
                    _ => Err("count: argument must be a list".into()),
                }
            }
            "min" => {
                let items = list_or_varargs(args, "min")?;
                let Some((first, rest)) = items.split_first() else {
                    return Ok(FeelValue::Null);
                };
                let mut minimum = first.clone();
                for item in rest {
                    match item.partial_cmp(&minimum) {
                        Some(std::cmp::Ordering::Less) => minimum = item.clone(),
                        Some(_) => {}
                        None => return Ok(FeelValue::Null),
                    }
                }
                Ok(minimum)
            }
            "max" => {
                let items = list_or_varargs(args, "max")?;
                let Some((first, rest)) = items.split_first() else {
                    return Ok(FeelValue::Null);
                };
                let mut maximum = first.clone();
                for item in rest {
                    match item.partial_cmp(&maximum) {
                        Some(std::cmp::Ordering::Greater) => maximum = item.clone(),
                        Some(_) => {}
                        None => return Ok(FeelValue::Null),
                    }
                }
                Ok(maximum)
            }
            "sum" => {
                let numbers = numeric_args(args, "sum")?;
                if numbers.is_empty() {
                    Ok(FeelValue::Null)
                } else {
                    Ok(FeelValue::Number(numbers.iter().sum()))
                }
            }
            "mean" | "avg" => {
                let numbers = numeric_args(args, "mean")?;
                if numbers.is_empty() {
                    Ok(FeelValue::Null)
                } else {
                    Ok(FeelValue::Number(
                        numbers.iter().sum::<f64>() / numbers.len() as f64,
                    ))
                }
            }
            "append" => {
                if args.len() < 2 {
                    return Err("append(list, items...) requires at least 2 args".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let mut result = items.clone();
                        result.extend(args[1..].iter().cloned());
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("append: first argument must be a list".into()),
                }
            }
            "flatten" => {
                if args.len() != 1 {
                    return Err("flatten(list) requires 1 argument".into());
                }
                fn flat(v: &FeelValue, out: &mut Vec<FeelValue>) {
                    if let FeelValue::List(items) = v {
                        for item in items {
                            flat(item, out);
                        }
                    } else {
                        out.push(v.clone());
                    }
                }
                match &args[0] {
                    FeelValue::List(_) => {
                        let mut result = Vec::new();
                        flat(&args[0], &mut result);
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("flatten: argument must be a list".into()),
                }
            }
            "distinct values" => {
                if args.len() != 1 {
                    return Err("distinct values(list) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let mut seen: Vec<FeelValue> = Vec::new();
                        for item in items {
                            if !seen.contains(item) {
                                seen.push(item.clone());
                            }
                        }
                        Ok(FeelValue::List(seen))
                    }
                    _ => Err("distinct values: argument must be a list".into()),
                }
            }
            "reverse" => {
                if args.len() != 1 {
                    return Err("reverse(list) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let mut result = items.clone();
                        result.reverse();
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("reverse: argument must be a list".into()),
                }
            }
            "sort" => {
                if args.is_empty() {
                    return Err("sort(list) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let mut result = items.clone();
                        if args.len() >= 2 {
                            if let FeelValue::Function(params, body) = &args[1] {
                                // DMN 1.5: sort with precedes function
                                result.sort_by(|a, b| {
                                    let mut new_ctx = ctx.clone();
                                    if !params.is_empty() {
                                        new_ctx.insert(params[0].clone(), a.clone());
                                    }
                                    if params.len() > 1 {
                                        new_ctx.insert(params[1].clone(), b.clone());
                                    }
                                    if let Ok(FeelValue::Bool(true)) =
                                        self.eval_expr(body, &new_ctx)
                                    {
                                        std::cmp::Ordering::Less
                                    } else {
                                        std::cmp::Ordering::Greater
                                    }
                                });
                            } else {
                                let ascending = args[1].to_bool().unwrap_or(true);
                                result.sort_by(|a, b| {
                                    let ord = a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
                                    if ascending { ord } else { ord.reverse() }
                                });
                            }
                        } else {
                            result.sort_by(|a, b| {
                                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("sort: argument must be a list".into()),
                }
            }
            "sublist" => {
                if args.len() < 2 {
                    return Err("sublist(list, start[, length]) requires at least 2 args".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let start_pos =
                            args[1].to_f64().ok_or("sublist: start must be number")? as isize;
                        let len = items.len();
                        let start_idx = if start_pos < 0 {
                            len as isize + start_pos
                        } else if start_pos == 0 {
                            0
                        } else {
                            start_pos - 1
                        };
                        if !(0..len as isize).contains(&start_idx) {
                            return Err("sublist: start position is out of range".into());
                        }
                        let start_idx = start_idx as usize;
                        let take = if args.len() >= 3 {
                            args[2].to_f64().ok_or("sublist: length must be number")? as usize
                        } else {
                            len
                        };
                        Ok(FeelValue::List(
                            items[start_idx..].iter().take(take).cloned().collect(),
                        ))
                    }
                    _ => Err("sublist: first argument must be a list".into()),
                }
            }
            "insert before" => {
                if args.len() != 3 {
                    return Err("insert before(list, position, newItem) requires 3 args".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let pos = args[1]
                            .to_f64()
                            .ok_or("insert before: position must be number")?
                            as isize;
                        let len = items.len();
                        let pos_idx = if pos <= 0 {
                            0
                        } else if pos > len as isize {
                            len
                        } else {
                            (pos - 1) as usize
                        };
                        let mut result = items.clone();
                        result.insert(pos_idx, args[2].clone());
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("insert before: first argument must be a list".into()),
                }
            }
            "remove" => {
                if args.len() != 2 {
                    return Err("remove(list, position) requires 2 args".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let pos =
                            args[1].to_f64().ok_or("remove: position must be number")? as isize;
                        let len = items.len();
                        if len == 0 {
                            return Ok(FeelValue::List(vec![]));
                        }
                        let pos_idx = if pos <= 0 {
                            0
                        } else if pos > len as isize {
                            len - 1
                        } else {
                            (pos - 1) as usize
                        };
                        let mut result = items.clone();
                        result.remove(pos_idx);
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("remove: first argument must be a list".into()),
                }
            }
            "list replace" => {
                if args.len() != 3 {
                    return Err("list replace(list, position, newItem) requires 3 args".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let pos = args[1]
                            .to_f64()
                            .ok_or("list replace: position must be number")?
                            as isize;
                        let len = items.len();
                        if len == 0 {
                            return Ok(FeelValue::List(vec![]));
                        }
                        let pos_idx = if pos <= 0 {
                            0
                        } else if pos > len as isize {
                            len - 1
                        } else {
                            (pos - 1) as usize
                        };
                        let mut result = items.clone();
                        result[pos_idx] = args[2].clone();
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("list replace: first argument must be a list".into()),
                }
            }
            "union" => {
                if args.is_empty() {
                    return Err("union requires at least 1 argument".into());
                }
                let mut result = Vec::new();
                for arg in args {
                    match arg {
                        FeelValue::List(items) => {
                            for item in items {
                                if !result.contains(item) {
                                    result.push(item.clone());
                                }
                            }
                        }
                        _ => return Err("union: every argument must be a list".into()),
                    }
                }
                Ok(FeelValue::List(result))
            }
            "product" => {
                let numbers = numeric_args(args, "product")?;
                if numbers.is_empty() {
                    return Ok(FeelValue::Null);
                }
                Ok(FeelValue::Number(numbers.iter().product()))
            }
            "median" => {
                let mut numbers = numeric_args(args, "median")?;
                if numbers.is_empty() {
                    return Ok(FeelValue::Null);
                }
                numbers.sort_by(f64::total_cmp);
                let length = numbers.len();
                Ok(FeelValue::Number(if length % 2 == 1 {
                    numbers[length / 2]
                } else {
                    (numbers[length / 2 - 1] + numbers[length / 2]) / 2.0
                }))
            }
            "mode" => {
                let numbers = numeric_args(args, "mode")?;
                if numbers.is_empty() {
                    return Ok(FeelValue::List(Vec::new()));
                }
                let mut counts: Vec<(f64, usize)> = Vec::new();
                for number in numbers {
                    if let Some((_, count)) = counts.iter_mut().find(|(value, _)| *value == number)
                    {
                        *count += 1;
                    } else {
                        counts.push((number, 1));
                    }
                }
                let maximum = counts.iter().map(|(_, count)| *count).max().unwrap_or(0);
                let mut modes: Vec<f64> = counts
                    .into_iter()
                    .filter_map(|(number, count)| (count == maximum).then_some(number))
                    .collect();
                modes.sort_by(f64::total_cmp);
                Ok(FeelValue::List(
                    modes.into_iter().map(FeelValue::Number).collect(),
                ))
            }
            "stddev" => {
                let numbers = numeric_args(args, "stddev")?;
                if numbers.len() < 2 {
                    return Ok(FeelValue::Null);
                }
                let mean = numbers.iter().sum::<f64>() / numbers.len() as f64;
                let variance = numbers.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
                    / (numbers.len() - 1) as f64;
                Ok(FeelValue::Number(variance.sqrt()))
            }
            "all" => {
                let items = list_or_varargs(args, "all")?;
                let mut has_null = false;
                for v in items {
                    match v {
                        FeelValue::Bool(false) => return Ok(FeelValue::Bool(false)),
                        FeelValue::Null => has_null = true,
                        FeelValue::Bool(true) => {}
                        _ => return Ok(FeelValue::Null),
                    }
                }
                if has_null {
                    Ok(FeelValue::Null)
                } else {
                    Ok(FeelValue::Bool(true))
                }
            }
            "any" => {
                let items = list_or_varargs(args, "any")?;
                let mut has_null = false;
                for v in items {
                    match v {
                        FeelValue::Bool(true) => return Ok(FeelValue::Bool(true)),
                        FeelValue::Null => has_null = true,
                        FeelValue::Bool(false) => {}
                        _ => return Ok(FeelValue::Null),
                    }
                }
                if has_null {
                    Ok(FeelValue::Null)
                } else {
                    Ok(FeelValue::Bool(false))
                }
            }
            "index of" => {
                if args.len() != 2 {
                    return Err("index of(list, item) requires 2 arguments".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let indices: Vec<FeelValue> = items
                            .iter()
                            .enumerate()
                            .filter(|(_, v)| **v == args[1])
                            .map(|(i, _)| FeelValue::Number((i + 1) as f64)) // DMN 1.5: 1-based
                            .collect();
                        Ok(FeelValue::List(indices))
                    }
                    _ => Err("index of: first argument must be a list".into()),
                }
            }
            "list contains" => {
                if args.len() != 2 {
                    return Err("list contains(list, item) requires 2 arguments".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        Ok(FeelValue::Bool(items.iter().any(|v| *v == args[1])))
                    }
                    _ => Err("list contains: first argument must be a list".into()),
                }
            }

            // ---- 数值函数 ----
            "abs" => {
                if args.len() != 1 {
                    return Err("abs(n) requires 1 argument".into());
                }
                args[0]
                    .to_f64()
                    .map(|n| FeelValue::Number(n.abs()))
                    .ok_or_else(|| "abs: argument must be a number".into())
            }
            "floor" => {
                if args.len() != 1 {
                    return Err("floor(n) requires 1 argument".into());
                }
                args[0]
                    .to_f64()
                    .map(|n| FeelValue::Number(n.floor()))
                    .ok_or_else(|| "floor: argument must be a number".into())
            }
            "ceiling" => {
                if args.len() != 1 {
                    return Err("ceiling(n) requires 1 argument".into());
                }
                args[0]
                    .to_f64()
                    .map(|n| FeelValue::Number(n.ceil()))
                    .ok_or_else(|| "ceiling: argument must be a number".into())
            }
            "round" => {
                if args.is_empty() {
                    return Err("round(n[, digits]) requires at least 1 argument".into());
                }
                let n = args[0].to_f64().ok_or("round: argument must be a number")?;
                let digits = if args.len() >= 2 {
                    args[1].to_f64().ok_or("round: digits must be a number")? as i32
                } else {
                    0
                };
                let factor = 10_f64.powi(digits);
                Ok(FeelValue::Number((n * factor).round() / factor))
            }
            "decimal" => {
                let (number, scale) = scale_arg(args, "decimal")?;
                Ok(scaled_round(number, scale, f64::round_ties_even))
            }
            "round up" => {
                let (number, scale) = scale_arg(args, "round up")?;
                Ok(scaled_round(number, scale, |value| {
                    if value.is_sign_negative() {
                        value.floor()
                    } else {
                        value.ceil()
                    }
                }))
            }
            "round down" => {
                let (number, scale) = scale_arg(args, "round down")?;
                Ok(scaled_round(number, scale, f64::trunc))
            }
            "round half up" => {
                let (number, scale) = scale_arg(args, "round half up")?;
                Ok(scaled_round(number, scale, f64::round))
            }
            "round half down" => {
                let (number, scale) = scale_arg(args, "round half down")?;
                Ok(scaled_round(number, scale, |value| {
                    let absolute = value.abs();
                    let lower = absolute.floor();
                    let rounded = if (absolute - lower - 0.5).abs() < f64::EPSILON {
                        lower
                    } else {
                        absolute.round()
                    };
                    rounded.copysign(value)
                }))
            }
            "odd" => {
                if args.len() != 1 {
                    return Err("odd(n) requires 1 argument".into());
                }
                let n = args[0].to_f64().ok_or("odd: argument must be a number")?;
                Ok(FeelValue::Bool((n as i64) % 2 != 0))
            }
            "even" => {
                if args.len() != 1 {
                    return Err("even(n) requires 1 argument".into());
                }
                let n = args[0].to_f64().ok_or("even: argument must be a number")?;
                Ok(FeelValue::Bool((n as i64) % 2 == 0))
            }
            "modulo" => {
                if args.len() != 2 {
                    return Err("modulo(a, b) requires 2 arguments".into());
                }
                match (args[0].to_f64(), args[1].to_f64()) {
                    (Some(a), Some(b)) if b != 0.0 => {
                        // DMN 1.5: result has same sign as divisor
                        let r = a - (a / b).floor() * b;
                        Ok(FeelValue::Number(r))
                    }
                    _ => Err("modulo: requires two numbers, divisor non-zero".into()),
                }
            }
            "sqrt" => {
                if args.len() != 1 {
                    return Err("sqrt(n) requires 1 argument".into());
                }
                match args[0].to_f64() {
                    Some(number) if number >= 0.0 && number.is_finite() => {
                        Ok(FeelValue::Number(number.sqrt()))
                    }
                    Some(_) => Ok(FeelValue::Null),
                    None => Err("sqrt: argument must be a number".into()),
                }
            }
            "log" => {
                if args.len() != 1 {
                    return Err("log(n) requires 1 argument".into());
                }
                args[0]
                    .to_f64()
                    .map(|n| FeelValue::Number(n.ln()))
                    .ok_or_else(|| "log: argument must be a number".into())
            }
            "exp" => {
                if args.len() != 1 {
                    return Err("exp(n) requires 1 argument".into());
                }
                match args[0].to_f64() {
                    Some(number) => {
                        let result = number.exp();
                        Ok(if result.is_finite() {
                            FeelValue::Number(result)
                        } else {
                            FeelValue::Null
                        })
                    }
                    None => Err("exp: argument must be a number".into()),
                }
            }
            /*"random" => {
                Ok(FeelValue::Number(random::<f64>()))
            }*/
            // ---- 类型转换 ----
            "number" => {
                if args.is_empty() {
                    return Err("number(x) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::Number(n) => Ok(FeelValue::Number(*n)),
                    FeelValue::String(s) => s
                        .parse::<f64>()
                        .map(FeelValue::Number)
                        .map_err(|e| format!("Cannot convert '{}' to number: {}", s, e)),
                    FeelValue::Bool(b) => Ok(FeelValue::Number(if *b { 1.0 } else { 0.0 })),
                    _ => Err(format!("Cannot convert {} to number", args[0])),
                }
            }
            "boolean" => {
                if args.is_empty() {
                    return Err("boolean(x) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::Bool(b) => Ok(FeelValue::Bool(*b)),
                    FeelValue::String(s) => match s.to_lowercase().as_str() {
                        "true" => Ok(FeelValue::Bool(true)),
                        "false" => Ok(FeelValue::Bool(false)),
                        _ => Err(format!("Cannot convert '{}' to boolean", s)),
                    },
                    _ => Err(format!("Cannot convert {} to boolean", args[0])),
                }
            }

            // ---- 日期函数 (简化) ----
            "now" => Ok(FeelValue::DateTime(
                chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            )),
            "today" => Ok(FeelValue::Date(
                chrono::Local::now().format("%Y-%m-%d").to_string(),
            )),
            "date and time" => {
                let value = match args {
                    [FeelValue::String(value)] | [FeelValue::DateTime(value)] => value.clone(),
                    [FeelValue::Date(date), FeelValue::Time(time)] => format!("{date}T{time}"),
                    _ => {
                        return Err(
                            "date and time requires a string/date-time, or a date and a time"
                                .into(),
                        );
                    }
                };
                validate_date_time(&value)?;
                Ok(FeelValue::DateTime(value))
            }
            "year" | "month" | "day" | "hour" | "minute" | "second" => {
                if args.len() != 1 {
                    return Err(format!("{}(d) requires 1 argument", name));
                }
                let input = match &args[0] {
                    FeelValue::Date(value)
                    | FeelValue::Time(value)
                    | FeelValue::DateTime(value)
                    | FeelValue::String(value) => value,
                    _ => return Err(format!("{name}: argument must be a temporal value")),
                };
                match name.to_lowercase().as_str() {
                    "year" => Ok(FeelValue::Number(parse_date(input, name)?.year() as f64)),
                    "month" => Ok(FeelValue::Number(parse_date(input, name)?.month() as f64)),
                    "day" => Ok(FeelValue::Number(parse_date(input, name)?.day() as f64)),
                    "hour" => Ok(FeelValue::Number(parse_time(input, name)?.hour() as f64)),
                    "minute" => Ok(FeelValue::Number(parse_time(input, name)?.minute() as f64)),
                    "second" => {
                        let time = parse_time(input, name)?;
                        Ok(FeelValue::Number(
                            time.second() as f64 + time.nanosecond() as f64 / 1_000_000_000.0,
                        ))
                    }
                    _ => unreachable!(),
                }
            }
            "day of week" => {
                if args.len() != 1 {
                    return Err("day of week(d) requires 1 argument".into());
                }
                let date = parse_date(&self.format_temporal(&args[0]), "day of week")?;
                Ok(FeelValue::String(
                    match date.weekday() {
                        chrono::Weekday::Mon => "Monday",
                        chrono::Weekday::Tue => "Tuesday",
                        chrono::Weekday::Wed => "Wednesday",
                        chrono::Weekday::Thu => "Thursday",
                        chrono::Weekday::Fri => "Friday",
                        chrono::Weekday::Sat => "Saturday",
                        chrono::Weekday::Sun => "Sunday",
                    }
                    .to_string(),
                ))
            }

            // ---- 上下文函数 ----
            "get entries" => {
                if args.len() != 1 {
                    return Err("get entries(ctx) requires 1 argument".into());
                }
                match &args[0] {
                    FeelValue::Context(map) => {
                        let entries: Vec<FeelValue> = map
                            .iter()
                            .map(|(k, v)| {
                                FeelValue::Context(
                                    vec![
                                        ("key".to_string(), FeelValue::String(k.clone())),
                                        ("value".to_string(), v.clone()),
                                    ]
                                    .into_iter()
                                    .collect(),
                                )
                            })
                            .collect();
                        Ok(FeelValue::List(entries))
                    }
                    _ => Err("get entries: argument must be a context".into()),
                }
            }
            "get value" => {
                if args.len() != 2 {
                    return Err("get value(ctx, key) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::Context(map), FeelValue::String(key)) => {
                        Ok(map.get(key).cloned().unwrap_or(FeelValue::Null))
                    }
                    _ => Err("get value: first arg must be context, second must be string".into()),
                }
            }

            // ---- Range functions (DMN 1.5 10.3.4.7) ----
            "before" | "after" | "meets" | "met by" | "overlaps" | "overlaps before"
            | "overlaps after" | "finishes" | "finished by" | "includes" | "during" | "starts"
            | "started by" | "coincides" => {
                Self::eval_range_relation(name.to_lowercase().as_str(), args)
            }

            // ---- Temporal functions (DMN 1.5 10.3.4.8) ----
            "day of year" => {
                if args.len() != 1 {
                    return Err("day of year(d) requires 1 argument".into());
                }
                let date = parse_date(&self.format_temporal(&args[0]), "day of year")?;
                Ok(FeelValue::Number(date.ordinal() as f64))
            }
            "month of year" => {
                if args.len() != 1 {
                    return Err("month of year(d) requires 1 argument".into());
                }
                let months = [
                    "January",
                    "February",
                    "March",
                    "April",
                    "May",
                    "June",
                    "July",
                    "August",
                    "September",
                    "October",
                    "November",
                    "December",
                ];
                let date = parse_date(&self.format_temporal(&args[0]), "month of year")?;
                Ok(FeelValue::String(
                    months[date.month0() as usize].to_string(),
                ))
            }
            "week of year" => {
                if args.len() != 1 {
                    return Err("week of year(d) requires 1 argument".into());
                }
                let date = parse_date(&self.format_temporal(&args[0]), "week of year")?;
                Ok(FeelValue::Number(date.iso_week().week() as f64))
            }

            // ---- Context functions ----
            "context" => {
                if args.len() != 1 {
                    return Err("context(entries) requires 1 arg".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let mut map = HashMap::new();
                        for item in items {
                            match item {
                                FeelValue::Context(entry_map) => {
                                    if let (Some(FeelValue::String(k)), Some(v)) =
                                        (entry_map.get("key"), entry_map.get("value"))
                                    {
                                        map.insert(k.clone(), v.clone());
                                    } else {
                                        return Ok(FeelValue::Null);
                                    }
                                }
                                _ => return Ok(FeelValue::Null),
                            }
                        }
                        Ok(FeelValue::Context(map))
                    }
                    _ => Err("context: argument must be a list of {key, value} entries".into()),
                }
            }
            "context put" => {
                if args.len() != 3 {
                    return Err("context put(ctx, key, value) requires 3 args".into());
                }
                match &args[0] {
                    FeelValue::Context(map) => {
                        let mut new_map = map.clone();
                        let keys = match &args[1] {
                            FeelValue::String(key) => vec![key.clone()],
                            FeelValue::List(keys) => keys
                                .iter()
                                .map(|key| match key {
                                    FeelValue::String(key) => Ok(key.clone()),
                                    _ => Err("context put: key path must contain strings".into()),
                                })
                                .collect::<Result<Vec<_>, String>>()?,
                            _ => {
                                return Err(
                                    "context put: key must be a string or list of strings".into()
                                );
                            }
                        };
                        context_put_path(&mut new_map, &keys, args[2].clone())?;
                        Ok(FeelValue::Context(new_map))
                    }
                    _ => Err("context put: first argument must be a context".into()),
                }
            }
            "context merge" => {
                if args.len() != 1 {
                    return Err("context merge(contexts) requires 1 arg".into());
                }
                match &args[0] {
                    FeelValue::List(items) => {
                        let mut map = HashMap::new();
                        for item in items {
                            match item {
                                FeelValue::Context(m) => {
                                    for (k, v) in m {
                                        map.insert(k.clone(), v.clone());
                                    }
                                }
                                _ => return Ok(FeelValue::Null),
                            }
                        }
                        Ok(FeelValue::Context(map))
                    }
                    _ => Err("context merge: argument must be a list of contexts".into()),
                }
            }

            // ---- Conversion functions (DMN 1.5 10.3.4.1) ----
            "date" => {
                if args.len() == 1 {
                    let input = match &args[0] {
                        FeelValue::String(value)
                        | FeelValue::Date(value)
                        | FeelValue::DateTime(value) => value,
                        _ => {
                            return Err(
                                "date: argument must be a string, date, or date and time".into()
                            );
                        }
                    };
                    let date = parse_date(input, "date")?;
                    Ok(FeelValue::Date(date.format("%Y-%m-%d").to_string()))
                } else if args.len() == 3 {
                    let year = integer_component(&args[0], "date: year")?;
                    let month = integer_component(&args[1], "date: month")?;
                    let day = integer_component(&args[2], "date: day")?;
                    let year = i32::try_from(year).map_err(|_| "date: year is out of range")?;
                    let month = u32::try_from(month).map_err(|_| "date: month is out of range")?;
                    let day = u32::try_from(day).map_err(|_| "date: day is out of range")?;
                    let date = NaiveDate::from_ymd_opt(year, month, day)
                        .ok_or("date: invalid calendar date")?;
                    Ok(FeelValue::Date(date.format("%Y-%m-%d").to_string()))
                } else {
                    Err("date: requires 1 or 3 arguments".into())
                }
            }
            "time" => {
                if args.len() == 1 {
                    let input = match &args[0] {
                        FeelValue::String(value)
                        | FeelValue::Time(value)
                        | FeelValue::DateTime(value) => value,
                        _ => {
                            return Err(
                                "time: argument must be a string, time, or date and time".into()
                            );
                        }
                    };
                    parse_time(input, "time")?;
                    Ok(FeelValue::Time(
                        input
                            .split_once('T')
                            .map_or(input.as_str(), |(_, time)| time)
                            .to_string(),
                    ))
                } else if args.len() == 3 {
                    let hour = integer_component(&args[0], "time: hour")?;
                    let minute = integer_component(&args[1], "time: minute")?;
                    let second = args[2]
                        .to_f64()
                        .filter(|value| value.is_finite() && (0.0..60.0).contains(value))
                        .ok_or("time: second must be at least 0 and less than 60")?;
                    let hour = u32::try_from(hour).map_err(|_| "time: hour is out of range")?;
                    let minute =
                        u32::try_from(minute).map_err(|_| "time: minute is out of range")?;
                    let value = format!("{hour:02}:{minute:02}:{}", format_second(second));
                    parse_time(&value, "time")?;
                    Ok(FeelValue::Time(value))
                } else {
                    Err("time: requires 1 or 3 arguments".into())
                }
            }
            "duration" => {
                if args.len() != 1 {
                    return Err("duration(s) requires 1 arg".into());
                }
                let value = match &args[0] {
                    FeelValue::String(value) | FeelValue::Duration(value) => value.clone(),
                    _ => return Err("duration: argument must be a string or duration".into()),
                };
                validate_duration(&value)?;
                Ok(FeelValue::Duration(value))
            }
            "years and months duration" => {
                if args.len() != 2 {
                    return Err("years and months duration(from, to) requires 2 args".into());
                }
                let from =
                    parse_date(&self.format_temporal(&args[0]), "years and months duration")?;
                let to = parse_date(&self.format_temporal(&args[1]), "years and months duration")?;
                let total_months =
                    (to.year() - from.year()) * 12 + to.month() as i32 - from.month() as i32;
                let sign = if total_months < 0 { "-" } else { "" };
                let absolute = total_months.unsigned_abs();
                let years = absolute / 12;
                let months = absolute % 12;
                if months == 0 {
                    Ok(FeelValue::Duration(format!("{sign}P{years}Y")))
                } else if years == 0 {
                    Ok(FeelValue::Duration(format!("{sign}P{months}M")))
                } else {
                    Ok(FeelValue::Duration(format!("{sign}P{years}Y{months}M")))
                }
            }
            "range" => {
                if args.len() != 1 {
                    return Err("range(string) requires 1 arg".into());
                }
                let s = self.format_temporal(&args[0]);
                // Parse range string like "[1..10)" or "(..10]" or "[2..)"
                Self::parse_range_string(&s)
            }

            // ---- 判断函数 ----
            "is defined" => {
                if args.len() != 1 {
                    return Err("is defined(x) requires 1 argument".into());
                }
                Ok(FeelValue::Bool(!matches!(args[0], FeelValue::Null)))
            }
            "is" => {
                if args.len() != 2 {
                    return Err("is(v1, v2) requires 2 arguments".into());
                }
                Ok(FeelValue::Bool(args[0] == args[1]))
            }
            "type of" => {
                if args.len() != 1 {
                    return Err("type of(x) requires 1 argument".into());
                }
                let type_name = match args[0] {
                    FeelValue::Null => "Null",
                    FeelValue::Bool(_) => "Boolean",
                    FeelValue::Number(_) => "Number",
                    FeelValue::String(_) => "String",
                    FeelValue::Date(_) => "date",
                    FeelValue::Time(_) => "time",
                    FeelValue::DateTime(_) => "date and time",
                    FeelValue::Duration(_) => "duration",
                    FeelValue::List(_) => "List",
                    FeelValue::Context(_) => "Context",
                    FeelValue::Range { .. } => "Range",
                    FeelValue::Function(_, _) => "Function",
                };
                Ok(FeelValue::String(type_name.to_string()))
            }

            // ---- 高阶函数 ----
            "filter" => {
                if args.len() != 2 {
                    return Err("filter(list, predicate) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::List(items), FeelValue::Function(params, body)) => {
                        let mut result = Vec::new();
                        for item in items {
                            let mut new_ctx = ctx.clone();
                            if !params.is_empty() {
                                new_ctx.insert(params[0].clone(), item.clone());
                            }
                            if self.eval_expr(body, &new_ctx)? == FeelValue::Bool(true) {
                                result.push(item.clone());
                            }
                        }
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("filter: first arg must be list, second must be function".into()),
                }
            }
            "map" => {
                if args.len() != 2 {
                    return Err("map(list, func) requires 2 arguments".into());
                }
                match (&args[0], &args[1]) {
                    (FeelValue::List(items), FeelValue::Function(params, body)) => {
                        let mut result = Vec::new();
                        for item in items {
                            let mut new_ctx = ctx.clone();
                            if !params.is_empty() {
                                new_ctx.insert(params[0].clone(), item.clone());
                            }
                            result.push(self.eval_expr(body, &new_ctx)?);
                        }
                        Ok(FeelValue::List(result))
                    }
                    _ => Err("map: first arg must be list, second must be function".into()),
                }
            }
            "reduce" => {
                if args.len() != 3 {
                    return Err("reduce(list, init, func) requires 3 arguments".into());
                }
                match (&args[0], &args[2]) {
                    (FeelValue::List(items), FeelValue::Function(params, body)) => {
                        let mut acc = args[1].clone();
                        for item in items {
                            let mut new_ctx = ctx.clone();
                            if !params.is_empty() {
                                new_ctx.insert(params[0].clone(), acc.clone());
                            }
                            if params.len() >= 2 {
                                new_ctx.insert(params[1].clone(), item.clone());
                            }
                            acc = self.eval_expr(body, &new_ctx)?;
                        }
                        Ok(acc)
                    }
                    _ => Err("reduce: first arg must be list, third must be function".into()),
                }
            }

            // ---- 自定义上下文中的函数 ----
            _ => {
                if let Some(FeelValue::Function(params, body)) = ctx.get(name) {
                    if params.len() != args.len() {
                        return Err(format!(
                            "Function '{}' expects {} arguments, got {}",
                            name,
                            params.len(),
                            args.len()
                        ));
                    }
                    let mut new_ctx = ctx.clone();
                    for (p, a) in params.iter().zip(args.iter()) {
                        new_ctx.insert(p.clone(), a.clone());
                    }
                    self.eval_expr(body, &new_ctx)
                } else {
                    Err(format!("Unknown function: {}", name))
                }
            }
        }
    }
}
