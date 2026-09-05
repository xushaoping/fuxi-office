use super::ast::*;
use super::value::FeelValue;
use crate::feel::parse_feel;
use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, FixedOffset, Local, NaiveDate, NaiveDateTime,
    NaiveTime, TimeZone, Timelike,
};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::OnceLock;

fn builtin_parameter_names(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "sort" => Some(&["list", "precedes"]),
        "substring" => Some(&["string", "start position", "length"]),
        "date" => Some(&["year", "month", "day"]),
        "time" => Some(&["hour", "minute", "second"]),
        "replace" => Some(&["input", "pattern", "replacement"]),
        "matches" => Some(&["input", "pattern"]),
        _ => None,
    }
}

// ============================================================
//  解释器 / 求值器
// ============================================================

pub struct FeelInterpreter {
    // 状态化解释器上下文：产品路径以 eval_expr 显式传 ctx，仅状态化 API（with_context/eval/eval_ast）读取
    #[cfg_attr(not(test), allow(dead_code))]
    context: HashMap<String, FeelValue>,
}

#[derive(Clone, Copy)]
struct NumericInterval {
    start: f64,
    end: f64,
    start_open: bool,
    end_open: bool,
}

impl FeelInterpreter {
    pub fn new() -> Self {
        FeelInterpreter {
            context: HashMap::new(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // 状态化解释器入口，当前仅 eval_feel_with（测试）使用
    pub fn with_context(ctx: HashMap<String, FeelValue>) -> Self {
        FeelInterpreter { context: ctx }
    }

    #[allow(dead_code)] // 可变状态设置：无调用者（含测试），状态化 API 预留
    pub fn set(&mut self, name: &str, value: FeelValue) {
        self.context.insert(name.to_string(), value);
    }

    /// 解析并执行 FEEL 表达式
    #[cfg_attr(not(test), allow(dead_code))] // 当前仅 eval_feel（测试）使用
    pub fn eval(&self, input: &str) -> Result<FeelValue, String> {
        let ast = parse_feel(input)?;
        self.eval_expr(&ast, &self.context)
    }

    /// 在当前上下文基础上，额外添加变量后执行
    #[allow(dead_code)] // 状态化扩展执行：无调用者，与 eval 对称保留
    pub fn eval_with(
        &self,
        input: &str,
        extra: &HashMap<String, FeelValue>,
    ) -> Result<FeelValue, String> {
        let mut ctx = self.context.clone();
        for (k, v) in extra {
            ctx.insert(k.clone(), v.clone());
        }
        let ast = parse_feel(input)?;
        self.eval_expr(&ast, &ctx)
    }

    #[allow(dead_code)] // AST 直接求值：无调用者，与 eval 对称保留
    pub fn eval_ast(&self, expr: &FeelExpr) -> Result<FeelValue, String> {
        self.eval_expr(expr, &self.context)
    }

    #[allow(dead_code)] // 状态化 AST 扩展求值：无调用者，预留 API
    pub fn eval_ast_with(
        &self,
        expr: &FeelExpr,
        extra: &HashMap<String, FeelValue>,
    ) -> Result<FeelValue, String> {
        let mut ctx = self.context.clone();
        ctx.extend(extra.clone());
        self.eval_expr(expr, &ctx)
    }

    pub(crate) fn eval_expr(
        &self,
        expr: &FeelExpr,
        ctx: &HashMap<String, FeelValue>,
    ) -> Result<FeelValue, String> {
        match expr {
            FeelExpr::Null => Ok(FeelValue::Null),
            FeelExpr::Bool(b) => Ok(FeelValue::Bool(*b)),
            FeelExpr::Number(n) => Ok(FeelValue::Number(*n)),
            FeelExpr::String(s) => Ok(FeelValue::String(s.clone())),
            FeelExpr::DateLit(d) => Ok(FeelValue::Date(d.clone())),
            FeelExpr::DateTimeLit(dt) => Ok(FeelValue::DateTime(dt.clone())),

            FeelExpr::Name(name) => ctx
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Undefined variable: {}", name)),

            FeelExpr::QualifiedName(parts) => {
                let first = parts.first().unwrap();
                let mut val = ctx
                    .get(first)
                    .cloned()
                    .ok_or_else(|| format!("Undefined variable: {}", first))?;
                for part in &parts[1..] {
                    val = self.access_property(&val, part)?;
                }
                Ok(val)
            }

            FeelExpr::Binary(left, op, right) => {
                // 短路 for 'in' with range
                if matches!(op, BinOp::In) {
                    return self.eval_in(left, right, ctx);
                }
                let lval = self.eval_expr(left, ctx)?;
                let rval = self.eval_expr(right, ctx)?;
                self.eval_binary_op(&lval, *op, &rval)
            }

            FeelExpr::Unary(op, expr) => {
                let val = self.eval_expr(expr, ctx)?;
                match op {
                    UnOp::Minus => match val.to_f64() {
                        Some(n) => Ok(FeelValue::Number(-n)),
                        None => Err(format!("Cannot negate non-number: {}", val)),
                    },
                    UnOp::Neg => Ok(val), // FEEL中没有单独的+一元，忽略
                }
            }

            FeelExpr::And(exprs) => {
                // DMN 1.5 ternary logic: null and false -> false, null and true -> null
                let mut has_null = false;
                for e in exprs {
                    let v = self.eval_expr(e, ctx)?;
                    match v {
                        FeelValue::Bool(false) => return Ok(FeelValue::Bool(false)),
                        FeelValue::Null => has_null = true,
                        FeelValue::Bool(true) => {}
                        _ => return Err(format!("Non-boolean in 'and': {}", v)),
                    }
                }
                if has_null {
                    Ok(FeelValue::Null)
                } else {
                    Ok(FeelValue::Bool(true))
                }
            }

            FeelExpr::Or(exprs) => {
                // DMN 1.5 ternary logic: null or true -> true, null or false -> null
                let mut has_null = false;
                for e in exprs {
                    let v = self.eval_expr(e, ctx)?;
                    match v {
                        FeelValue::Bool(true) => return Ok(FeelValue::Bool(true)),
                        FeelValue::Null => has_null = true,
                        FeelValue::Bool(false) => {}
                        _ => return Err(format!("Non-boolean in 'or': {}", v)),
                    }
                }
                if has_null {
                    Ok(FeelValue::Null)
                } else {
                    Ok(FeelValue::Bool(false))
                }
            }

            FeelExpr::Not(expr) => {
                let v = self.eval_expr(expr, ctx)?;
                match v {
                    FeelValue::Bool(b) => Ok(FeelValue::Bool(!b)),
                    FeelValue::Null => Ok(FeelValue::Null),
                    _ => Ok(FeelValue::Null), // DMN 1.5 ternary: invalid -> null
                }
            }

            FeelExpr::Interval {
                start,
                end,
                start_open,
                end_open,
            } => {
                let s = self.eval_expr(start, ctx)?;
                let e = self.eval_expr(end, ctx)?;
                Ok(FeelValue::Range {
                    start: Box::new(s),
                    end: Box::new(e),
                    start_open: *start_open,
                    end_open: *end_open,
                })
            }

            FeelExpr::List(items) => {
                let mut result = Vec::new();
                for item in items {
                    result.push(self.eval_expr(item, ctx)?);
                }
                Ok(FeelValue::List(result))
            }

            FeelExpr::ForExpr { var, iter, body } => {
                let iter_val = self.eval_expr(iter, ctx)?;
                let items = self.expand_iterable(&iter_val)?;
                let mut result = Vec::new();
                for item in items {
                    let mut new_ctx = ctx.clone();
                    new_ctx.insert(var.clone(), item.clone());
                    result.push(self.eval_expr(body, &new_ctx)?);
                }
                Ok(FeelValue::List(result))
            }

            FeelExpr::Quantified {
                kind,
                var,
                iter,
                satisfies,
            } => {
                let iter_val = self.eval_expr(iter, ctx)?;
                let items = self.expand_iterable(&iter_val)?;
                match kind {
                    QuantKind::Some => {
                        let mut has_null = false;
                        for item in items {
                            let mut new_ctx = ctx.clone();
                            new_ctx.insert(var.clone(), item.clone());
                            let v = self.eval_expr(satisfies, &new_ctx)?;
                            match v {
                                FeelValue::Bool(true) => return Ok(FeelValue::Bool(true)),
                                FeelValue::Bool(false) => {}
                                _ => has_null = true,
                            }
                        }
                        Ok(if has_null {
                            FeelValue::Null
                        } else {
                            FeelValue::Bool(false)
                        })
                    }
                    QuantKind::Every => {
                        let mut has_null = false;
                        for item in items {
                            let mut new_ctx = ctx.clone();
                            new_ctx.insert(var.clone(), item.clone());
                            let v = self.eval_expr(satisfies, &new_ctx)?;
                            match v {
                                FeelValue::Bool(false) => return Ok(FeelValue::Bool(false)),
                                FeelValue::Bool(true) => {}
                                _ => has_null = true,
                            }
                        }
                        Ok(if has_null {
                            FeelValue::Null
                        } else {
                            FeelValue::Bool(true)
                        })
                    }
                }
            }

            FeelExpr::IfExpr { cond, then_, else_ } => {
                let cv = self.eval_expr(cond, ctx)?;
                if cv == FeelValue::Bool(true) {
                    self.eval_expr(then_, ctx)
                } else {
                    self.eval_expr(else_, ctx)
                }
            }

            FeelExpr::Call(name, args) => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg, ctx)?);
                }
                self.call_function(name, &arg_vals, ctx)
            }

            FeelExpr::Context(entries) => {
                let mut map = HashMap::new();
                let mut entry_ctx = ctx.clone();
                for (key, val_expr) in entries {
                    let value = self.eval_expr(val_expr, &entry_ctx)?;
                    entry_ctx.insert(key.clone(), value.clone());
                    map.insert(key.clone(), value);
                }
                Ok(FeelValue::Context(map))
            }

            FeelExpr::Filter(base, index) => {
                let base_val = self.eval_expr(base, ctx)?;
                match &base_val {
                    FeelValue::List(items) => {
                        if let Ok(FeelValue::Number(n)) = self.eval_expr(index, ctx) {
                            // DMN 1.5: 1-based indexing, [-1] is last element, [0] returns null
                            if !n.is_finite() || n.fract() != 0.0 {
                                return Ok(FeelValue::Null);
                            }
                            let idx = n as isize;
                            if idx == 0 {
                                return Ok(FeelValue::Null);
                            }
                            if idx < 0 {
                                let abs_idx = (-idx) as usize;
                                if abs_idx <= items.len() && abs_idx > 0 {
                                    return Ok(items[items.len() - abs_idx].clone());
                                } else {
                                    return Ok(FeelValue::Null);
                                }
                            } else {
                                let idx = (idx - 1) as usize; // convert 1-based to 0-based
                                return Ok(items.get(idx).cloned().unwrap_or(FeelValue::Null));
                            }
                        }
                        // 过滤: 返回满足条件的元素列表
                        let mut result = Vec::new();
                        for item in items {
                            let mut new_ctx = ctx.clone();
                            if let FeelValue::Context(entries) = item {
                                new_ctx.extend(entries.clone());
                                if !entries.contains_key("item") {
                                    new_ctx.insert("item".into(), item.clone());
                                }
                            } else {
                                new_ctx.insert("item".into(), item.clone());
                            }
                            let v = self.eval_expr(index, &new_ctx)?;
                            if v == FeelValue::Bool(true) {
                                result.push(item.clone());
                            }
                        }
                        Ok(FeelValue::List(result))
                    }
                    _ => Err(format!("Cannot filter/index: {}", base_val)),
                }
            }

            FeelExpr::Path(base, prop) => {
                let base_val = self.eval_expr(base, ctx)?;
                self.access_property(&base_val, prop)
            }

            FeelExpr::Index(base, idx) => {
                let base_val = self.eval_expr(base, ctx)?;
                let idx_val = self.eval_expr(idx, ctx)?;
                match (&base_val, &idx_val) {
                    (FeelValue::List(items), FeelValue::Number(n)) => {
                        // DMN 1.5: 1-based indexing
                        if !n.is_finite() || n.fract() != 0.0 {
                            return Ok(FeelValue::Null);
                        }
                        let idx = *n as isize;
                        if idx == 0 {
                            return Ok(FeelValue::Null);
                        }
                        if idx < 0 {
                            let abs_idx = (-idx) as usize;
                            if abs_idx <= items.len() && abs_idx > 0 {
                                Ok(items[items.len() - abs_idx].clone())
                            } else {
                                Ok(FeelValue::Null)
                            }
                        } else {
                            let idx = (idx - 1) as usize;
                            Ok(items.get(idx).cloned().unwrap_or(FeelValue::Null))
                        }
                    }
                    (FeelValue::Context(map), FeelValue::String(key)) => {
                        Ok(map.get(key).cloned().unwrap_or(FeelValue::Null))
                    }
                    _ => Err(format!("Cannot index: {}", base_val)),
                }
            }

            FeelExpr::FunctionDef(params, body) => {
                Ok(FeelValue::Function(params.clone(), *body.clone()))
            }

            // DMN 1.5 new expression types
            FeelExpr::Between { value, low, high } => {
                let v = self.eval_expr(value, ctx)?;
                let l = self.eval_expr(low, ctx)?;
                let h = self.eval_expr(high, ctx)?;
                let ge_low = self.eval_binary_op(&v, BinOp::Gte, &l)?;
                let le_high = self.eval_binary_op(&v, BinOp::Lte, &h)?;
                Ok(Self::ternary_and(&ge_low, &le_high))
            }

            FeelExpr::InstanceOf { value, type_name } => {
                let v = self.eval_expr(value, ctx)?;
                let type_name = type_name.to_lowercase();
                let matches = matches!(
                    (type_name.as_str(), &v),
                    ("number", FeelValue::Number(_))
                        | ("string", FeelValue::String(_))
                        | ("boolean", FeelValue::Bool(_))
                        | ("date", FeelValue::Date(_))
                        | ("time", FeelValue::Time(_))
                        | ("date and time" | "datetime", FeelValue::DateTime(_))
                        | (
                            "duration"
                                | "yearmonthduration"
                                | "daytimeduration"
                                | "years and months duration"
                                | "days and time duration",
                            FeelValue::Duration(_)
                        )
                        | ("list", FeelValue::List(_))
                        | ("context", FeelValue::Context(_))
                        | ("range", FeelValue::Range { .. })
                        | ("function", FeelValue::Function(_, _))
                        | ("null", FeelValue::Null)
                );
                Ok(FeelValue::Bool(matches))
            }

            FeelExpr::IfThenElse { cond, then_, else_ } => {
                let cv = self.eval_expr(cond, ctx)?;
                if cv == FeelValue::Bool(true) {
                    self.eval_expr(then_, ctx)
                } else {
                    self.eval_expr(else_, ctx)
                }
            }

            FeelExpr::AtLiteral(content) => {
                if content.contains('T') {
                    Self::parse_date_time(content)?;
                    Ok(FeelValue::DateTime(content.clone()))
                } else if content.as_bytes().get(4) == Some(&b'-')
                    && content.as_bytes().get(7) == Some(&b'-')
                {
                    Self::parse_date(content)?;
                    Ok(FeelValue::Date(content.clone()))
                } else {
                    Self::parse_time(content)?;
                    Ok(FeelValue::Time(content.clone()))
                }
            }

            FeelExpr::Is(left, right) => {
                let l = self.eval_expr(left, ctx)?;
                let r = self.eval_expr(right, ctx)?;
                Ok(FeelValue::Bool(l == r))
            }

            FeelExpr::IsNot(left, right) => {
                let l = self.eval_expr(left, ctx)?;
                let r = self.eval_expr(right, ctx)?;
                Ok(FeelValue::Bool(l != r))
            }

            FeelExpr::InUnaryTests { value, tests } => {
                let v = self.eval_expr(value, ctx)?;
                let mut has_null = false;
                for test in tests {
                    let matches = match test {
                        UnaryTest::LessThan(expr) => {
                            let bound = self.eval_expr(expr, ctx)?;
                            Self::compare(&v, &bound).map(|order| order == Ordering::Less)
                        }
                        UnaryTest::LessOrEqual(expr) => {
                            let bound = self.eval_expr(expr, ctx)?;
                            Self::compare(&v, &bound).map(|order| order != Ordering::Greater)
                        }
                        UnaryTest::GreaterThan(expr) => {
                            let bound = self.eval_expr(expr, ctx)?;
                            Self::compare(&v, &bound).map(|order| order == Ordering::Greater)
                        }
                        UnaryTest::GreaterOrEqual(expr) => {
                            let bound = self.eval_expr(expr, ctx)?;
                            Self::compare(&v, &bound).map(|order| order != Ordering::Less)
                        }
                        UnaryTest::Range(range_expr) => {
                            let range_val = self.eval_expr(range_expr, ctx)?;
                            Self::contains_value(&range_val, &v)
                        }
                        UnaryTest::Value(val_expr) => {
                            let val = self.eval_expr(val_expr, ctx)?;
                            Self::contains_value(&val, &v)
                        }
                    };
                    match matches {
                        Some(true) => return Ok(FeelValue::Bool(true)),
                        Some(false) => {}
                        None => has_null = true,
                    }
                }
                Ok(if has_null {
                    FeelValue::Null
                } else {
                    FeelValue::Bool(false)
                })
            }

            FeelExpr::CallNamed { name, args } => {
                let mut named_values = HashMap::new();
                for (parameter, value_expression) in args {
                    let value = self.eval_expr(value_expression, ctx)?;
                    if named_values.insert(parameter.clone(), value).is_some() {
                        return Err(format!(
                            "duplicate named parameter '{parameter}' for {name}"
                        ));
                    }
                }
                let parameters = builtin_parameter_names(name)
                    .ok_or_else(|| format!("named parameters are not supported for {name}"))?;
                if let Some(parameter) = named_values
                    .keys()
                    .find(|parameter| !parameters.contains(&parameter.as_str()))
                {
                    return Err(format!("unknown named parameter '{parameter}' for {name}"));
                }
                let mut values = Vec::with_capacity(parameters.len());
                for parameter in parameters {
                    let Some(value) = named_values.remove(*parameter) else {
                        return Err(format!("missing named parameter '{parameter}' for {name}"));
                    };
                    values.push(value);
                }
                self.call_function(name, &values, ctx)
            }
        }
    }

    fn access_property(&self, val: &FeelValue, prop: &str) -> Result<FeelValue, String> {
        match val {
            FeelValue::Context(map) => Ok(map.get(prop).cloned().unwrap_or(FeelValue::Null)),
            FeelValue::List(items) => match prop {
                "length" => Ok(FeelValue::Number(items.len() as f64)),
                _ => Ok(FeelValue::List(
                    items
                        .iter()
                        .map(|item| match item {
                            FeelValue::Context(map) => {
                                map.get(prop).cloned().unwrap_or(FeelValue::Null)
                            }
                            _ => FeelValue::Null,
                        })
                        .collect(),
                )),
            },
            FeelValue::String(s) => match prop {
                "length" => Ok(FeelValue::Number(s.chars().count() as f64)),
                _ => Err(format!("String has no property '{}'", prop)),
            },
            FeelValue::Date(d) => {
                // Date properties
                match prop {
                    "year" => self.extract_date_part(d, "year"),
                    "month" => self.extract_date_part(d, "month"),
                    "day" => self.extract_date_part(d, "day"),
                    _ => Err(format!("Date has no property '{}'", prop)),
                }
            }
            FeelValue::DateTime(dt) => match prop {
                "year" => self.extract_date_part(dt, "year"),
                "month" => self.extract_date_part(dt, "month"),
                "day" => self.extract_date_part(dt, "day"),
                "hour" => self.extract_date_part(dt, "hour"),
                "minute" => self.extract_date_part(dt, "minute"),
                "second" => self.extract_date_part(dt, "second"),
                _ => Err(format!("DateTime has no property '{}'", prop)),
            },
            FeelValue::Time(time) => match prop {
                "hour" => self.extract_date_part(time, "hour"),
                "minute" => self.extract_date_part(time, "minute"),
                "second" => self.extract_date_part(time, "second"),
                _ => Err(format!("Time has no property '{}'", prop)),
            },
            FeelValue::Duration(d) => match prop {
                "years" | "months" | "days" | "hours" | "minutes" | "seconds" => {
                    self.extract_duration_part(d, prop)
                }
                _ => Err(format!("Duration has no property '{}'", prop)),
            },
            _ => Err(format!("Cannot access property '{}' of {}", prop, val)),
        }
    }

    fn extract_date_part(&self, d: &str, part: &str) -> Result<FeelValue, String> {
        let value = match part {
            "year" => Self::parse_date(d)?.year() as f64,
            "month" => Self::parse_date(d)?.month() as f64,
            "day" => Self::parse_date(d)?.day() as f64,
            "hour" => Self::parse_time(d)?.hour() as f64,
            "minute" => Self::parse_time(d)?.minute() as f64,
            "second" => {
                let time = Self::parse_time(d)?;
                time.second() as f64 + time.nanosecond() as f64 / 1_000_000_000.0
            }
            _ => return Err(format!("Unknown temporal property '{part}'")),
        };
        Ok(FeelValue::Number(value))
    }

    fn extract_duration_part(&self, duration: &str, part: &str) -> Result<FeelValue, String> {
        static YEAR_MONTH: OnceLock<regex::Regex> = OnceLock::new();
        static DAY_TIME: OnceLock<regex::Regex> = OnceLock::new();
        let year_month = YEAR_MONTH.get_or_init(|| {
            regex::Regex::new(r"^(-)?P(?:(\d+)Y)?(?:(\d+)M)?$")
                .expect("year-month duration regex is valid")
        });
        let day_time = DAY_TIME.get_or_init(|| {
            regex::Regex::new(
                r"^(-)?P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?)?$",
            )
            .expect("day-time duration regex is valid")
        });
        let parse = |captures: &regex::Captures<'_>, index| -> Result<f64, String> {
            captures
                .get(index)
                .map(|value| value.as_str().parse::<f64>())
                .transpose()
                .map_err(|error| format!("Invalid duration '{duration}': {error}"))
                .map(|value| value.unwrap_or(0.0))
        };
        let value = if let Some(captures) = year_month.captures(duration) {
            let sign = if captures.get(1).is_some() { -1.0 } else { 1.0 };
            let total_months = parse(&captures, 2)? * 12.0 + parse(&captures, 3)?;
            let value = match part {
                "years" => (total_months / 12.0).trunc(),
                "months" => total_months % 12.0,
                "days" | "hours" | "minutes" | "seconds" => 0.0,
                _ => return Err(format!("Unknown duration property '{part}'")),
            };
            sign * value
        } else if let Some(captures) = day_time.captures(duration) {
            let sign = if captures.get(1).is_some() { -1.0 } else { 1.0 };
            let total_seconds = parse(&captures, 2)? * 86_400.0
                + parse(&captures, 3)? * 3_600.0
                + parse(&captures, 4)? * 60.0
                + parse(&captures, 5)?;
            let value = match part {
                "years" | "months" => 0.0,
                "days" => (total_seconds / 86_400.0).trunc(),
                "hours" => ((total_seconds % 86_400.0) / 3_600.0).trunc(),
                "minutes" => ((total_seconds % 3_600.0) / 60.0).trunc(),
                "seconds" => total_seconds % 60.0,
                _ => return Err(format!("Unknown duration property '{part}'")),
            };
            sign * value
        } else {
            return Err(format!("Invalid duration '{duration}'"));
        };
        Ok(FeelValue::Number(value))
    }

    fn parse_date(value: &str) -> Result<NaiveDate, String> {
        let date = value.split('T').next().unwrap_or(value);
        NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|error| format!("Invalid date '{value}': {error}"))
    }

    fn parse_time(value: &str) -> Result<NaiveTime, String> {
        let time = value.split_once('T').map_or(value, |(_, time)| time);
        if let Ok(date_time) = DateTime::parse_from_rfc3339(&format!("1970-01-01T{time}")) {
            return Ok(date_time.time());
        }
        NaiveTime::parse_from_str(time, "%H:%M:%S%.f")
            .map_err(|error| format!("Invalid time '{value}': {error}"))
    }

    fn parse_date_time(value: &str) -> Result<(), String> {
        if DateTime::parse_from_rfc3339(value).is_ok()
            || NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
        {
            Ok(())
        } else {
            Err(format!("Invalid date and time '{value}'"))
        }
    }

    fn parse_datetime_for_arithmetic(
        value: &str,
        fallback_offset: Option<FixedOffset>,
    ) -> Result<DateTime<FixedOffset>, String> {
        if let Ok(value) = DateTime::parse_from_rfc3339(value) {
            return Ok(value);
        }
        let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
            .map_err(|error| format!("Invalid date and time '{value}': {error}"))?;
        // `datetime-local` inputs intentionally omit an offset. If the other
        // operand carries one, use it so arithmetic is independent of the
        // server's timezone. With two local values, use the process timezone.
        match fallback_offset {
            Some(offset) => offset
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| format!("Invalid local date and time '{value}'")),
            None => Local
                .from_local_datetime(&naive)
                .single()
                .map(|value| value.fixed_offset())
                .ok_or_else(|| format!("Invalid local date and time '{value}'")),
        }
    }

    fn parse_datetime_pair_for_arithmetic(
        left: &str,
        right: &str,
    ) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>), String> {
        let left_zoned = DateTime::parse_from_rfc3339(left);
        let right_zoned = DateTime::parse_from_rfc3339(right);
        match (left_zoned, right_zoned) {
            (Ok(left), Ok(right)) => Ok((left, right)),
            (Ok(left), Err(_)) => Ok((
                left,
                Self::parse_datetime_for_arithmetic(right, Some(*left.offset()))?,
            )),
            (Err(_), Ok(right)) => Ok((
                Self::parse_datetime_for_arithmetic(left, Some(*right.offset()))?,
                right,
            )),
            (Err(_), Err(_)) => Ok((
                Self::parse_datetime_for_arithmetic(left, None)?,
                Self::parse_datetime_for_arithmetic(right, None)?,
            )),
        }
    }

    fn format_day_time_duration(duration: ChronoDuration) -> String {
        let negative = duration < ChronoDuration::zero();
        let duration = if negative { -duration } else { duration };
        let seconds = duration.num_seconds();
        let nanos = (duration - ChronoDuration::seconds(seconds))
            .num_nanoseconds()
            .unwrap_or_default();
        let days = seconds / 86_400;
        let hours = (seconds % 86_400) / 3_600;
        let minutes = (seconds % 3_600) / 60;
        let whole_seconds = seconds % 60;
        let mut value = String::new();
        if days > 0 {
            value.push_str(&format!("{days}D"));
        }
        if hours > 0 || minutes > 0 || whole_seconds > 0 || nanos > 0 || value.is_empty() {
            value.push('T');
            if hours > 0 {
                value.push_str(&format!("{hours}H"));
            }
            if minutes > 0 {
                value.push_str(&format!("{minutes}M"));
            }
            if nanos > 0 {
                let fraction = format!("{nanos:09}").trim_end_matches('0').to_string();
                value.push_str(&format!("{whole_seconds}.{fraction}S"));
            } else {
                value.push_str(&format!("{whole_seconds}S"));
            }
        }
        format!("{}P{value}", if negative { "-" } else { "" })
    }

    fn expand_iterable(&self, val: &FeelValue) -> Result<Vec<FeelValue>, String> {
        match val {
            FeelValue::List(items) => Ok(items.clone()),
            FeelValue::Range {
                start,
                end,
                start_open,
                end_open,
            } => {
                let s = start.to_f64().ok_or("Range start must be number")? as i64;
                let e = end.to_f64().ok_or("Range end must be number")? as i64;
                let items: Vec<FeelValue> = if *start_open && *end_open {
                    (s + 1..e).map(|n| FeelValue::Number(n as f64)).collect()
                } else if *start_open {
                    (s + 1..=e).map(|n| FeelValue::Number(n as f64)).collect()
                } else if *end_open {
                    (s..e).map(|n| FeelValue::Number(n as f64)).collect()
                } else {
                    (s..=e).map(|n| FeelValue::Number(n as f64)).collect()
                };
                Ok(items)
            }
            _ => Err(format!("Cannot iterate over: {}", val)),
        }
    }

    pub(crate) fn format_temporal(&self, v: &FeelValue) -> String {
        match v {
            FeelValue::String(s)
            | FeelValue::Date(s)
            | FeelValue::Time(s)
            | FeelValue::DateTime(s)
            | FeelValue::Duration(s) => s.clone(),
            other => format!("{}", other),
        }
    }

    fn numeric_interval(value: &FeelValue) -> Option<NumericInterval> {
        match value {
            FeelValue::Number(point) if point.is_finite() => Some(NumericInterval {
                start: *point,
                end: *point,
                start_open: false,
                end_open: false,
            }),
            FeelValue::Range {
                start,
                end,
                start_open,
                end_open,
            } => {
                let start = start.to_f64()?;
                let end = end.to_f64()?;
                (start.is_finite() && end.is_finite()).then_some(NumericInterval {
                    start,
                    end,
                    start_open: *start_open,
                    end_open: *end_open,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn eval_range_relation(name: &str, args: &[FeelValue]) -> Result<FeelValue, String> {
        if args.len() != 2 {
            return Err(format!("{name} requires 2 arguments"));
        }
        let first = Self::numeric_interval(&args[0])
            .ok_or_else(|| format!("{name}: first argument must be a numeric point or range"))?;
        let second = Self::numeric_interval(&args[1])
            .ok_or_else(|| format!("{name}: second argument must be a numeric point or range"))?;

        fn before(first: NumericInterval, second: NumericInterval) -> bool {
            first.end < second.start
                || (first.end == second.start && (first.end_open || second.start_open))
        }
        fn starts_before(first: NumericInterval, second: NumericInterval) -> bool {
            first.start < second.start
                || (first.start == second.start && !first.start_open && second.start_open)
        }
        fn ends_before(first: NumericInterval, second: NumericInterval) -> bool {
            first.end < second.end
                || (first.end == second.end && first.end_open && !second.end_open)
        }
        fn includes(outer: NumericInterval, inner: NumericInterval) -> bool {
            let includes_start = outer.start < inner.start
                || (outer.start == inner.start && (!outer.start_open || inner.start_open));
            let includes_end = outer.end > inner.end
                || (outer.end == inner.end && (!outer.end_open || inner.end_open));
            includes_start && includes_end
        }

        let result = match name {
            "before" => before(first, second),
            "after" => before(second, first),
            "meets" => first.end == second.start && !first.end_open && !second.start_open,
            "met by" => first.start == second.end && !first.start_open && !second.end_open,
            "overlaps" => !before(first, second) && !before(second, first),
            "overlaps before" => {
                starts_before(first, second) && !before(first, second) && ends_before(first, second)
            }
            "overlaps after" => {
                starts_before(second, first) && !before(second, first) && ends_before(second, first)
            }
            "finishes" => {
                first.end == second.end
                    && first.end_open == second.end_open
                    && starts_before(second, first)
            }
            "finished by" => {
                first.end == second.end
                    && first.end_open == second.end_open
                    && starts_before(first, second)
            }
            "includes" => includes(first, second),
            "during" => includes(second, first),
            "starts" => {
                first.start == second.start
                    && first.start_open == second.start_open
                    && ends_before(first, second)
            }
            "started by" => {
                first.start == second.start
                    && first.start_open == second.start_open
                    && ends_before(second, first)
            }
            "coincides" => {
                first.start == second.start
                    && first.end == second.end
                    && first.start_open == second.start_open
                    && first.end_open == second.end_open
            }
            _ => return Err(format!("unknown range relation '{name}'")),
        };
        Ok(FeelValue::Bool(result))
    }

    pub(crate) fn parse_range_string(s: &str) -> Result<FeelValue, String> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() < 2 {
            return Err("Invalid range string".into());
        }
        let start_open = chars[0] == '(';
        let end_open = *chars.last().unwrap() == ')';
        if !matches!(chars[0], '(' | '[') || !matches!(chars.last().unwrap(), ')' | ']') {
            return Err("Invalid range string delimiters".into());
        }
        let inner = &s[1..s.len() - 1];
        if let Some(dotdot_pos) = inner.find("..") {
            let start_str = &inner[..dotdot_pos];
            let end_str = &inner[dotdot_pos + 2..];
            let start = if start_str.is_empty() {
                FeelValue::Null
            } else {
                Self::parse_range_endpoint(start_str)
            };
            let end = if end_str.is_empty() {
                FeelValue::Null
            } else {
                Self::parse_range_endpoint(end_str)
            };
            Ok(FeelValue::Range {
                start: Box::new(start),
                end: Box::new(end),
                start_open,
                end_open,
            })
        } else {
            Err("Invalid range: missing '..'".into())
        }
    }

    fn parse_range_endpoint(s: &str) -> FeelValue {
        if let Ok(n) = s.parse::<f64>() {
            return FeelValue::Number(n);
        }
        if s.starts_with('"') && s.ends_with('"') {
            return FeelValue::String(s[1..s.len() - 1].to_string());
        }
        if s.starts_with('@') && s.ends_with('"') && s.ends_with('"') {
            let inner = &s[2..s.len() - 1];
            if inner.contains('T') {
                return FeelValue::DateTime(inner.to_string());
            }
            return FeelValue::Date(inner.to_string());
        }
        FeelValue::String(s.to_string())
    }

    fn eval_in(
        &self,
        left: &FeelExpr,
        right: &FeelExpr,
        ctx: &HashMap<String, FeelValue>,
    ) -> Result<FeelValue, String> {
        let l_val = self.eval_expr(left, ctx)?;
        let r_val = self.eval_expr(right, ctx)?;
        match Self::contains_value(&r_val, &l_val) {
            Some(value) => Ok(FeelValue::Bool(value)),
            None if matches!(r_val, FeelValue::List(_) | FeelValue::Range { .. }) => {
                Ok(FeelValue::Null)
            }
            None => Err(format!("'in' requires a list or range, got: {}", r_val)),
        }
    }

    fn eval_binary_op(&self, l: &FeelValue, op: BinOp, r: &FeelValue) -> Result<FeelValue, String> {
        match op {
            BinOp::Add => match (l, r) {
                (FeelValue::Number(a), FeelValue::Number(b)) => Ok(FeelValue::Number(a + b)),
                (FeelValue::String(a), FeelValue::String(b)) => {
                    Ok(FeelValue::String(format!("{}{}", a, b)))
                }
                _ => Err(format!("Cannot add {} and {}", l, r)),
            },
            BinOp::Sub => match (l, r) {
                (FeelValue::Date(a), FeelValue::Date(b)) => {
                    let d1 = NaiveDate::parse_from_str(a, "%Y-%m-%d")
                        .map_err(|e| format!("Invalid date {}: {}", a, e))?;
                    let d2 = NaiveDate::parse_from_str(b, "%Y-%m-%d")
                        .map_err(|e| format!("Invalid date {}: {}", b, e))?;
                    let days = (d1 - d2).num_days();
                    Ok(FeelValue::Duration(if days < 0 {
                        format!("-P{}D", days.unsigned_abs())
                    } else {
                        format!("P{days}D")
                    }))
                }
                (FeelValue::DateTime(a), FeelValue::DateTime(b)) => {
                    let (left, right) = Self::parse_datetime_pair_for_arithmetic(a, b)?;
                    let duration = left.signed_duration_since(right);
                    Ok(FeelValue::Duration(Self::format_day_time_duration(
                        duration,
                    )))
                }
                _ => match (l.to_f64(), r.to_f64()) {
                    (Some(a), Some(b)) => Ok(FeelValue::Number(a - b)),
                    _ => Err(format!("Cannot subtract {} and {}", l, r)),
                },
            },
            BinOp::Mul => match (l.to_f64(), r.to_f64()) {
                (Some(a), Some(b)) => Ok(FeelValue::Number(a * b)),
                _ => Err(format!("Cannot multiply {} and {}", l, r)),
            },
            BinOp::Div => match (l.to_f64(), r.to_f64()) {
                (Some(_), Some(0.0)) => Err("Division by zero".into()),
                (Some(a), Some(b)) => Ok(FeelValue::Number(a / b)),
                _ => Err(format!("Cannot divide {} and {}", l, r)),
            },
            BinOp::Pow => match (l.to_f64(), r.to_f64()) {
                (Some(a), Some(b)) => Ok(FeelValue::Number(a.powf(b))),
                _ => Err(format!("Cannot exponentiate {} and {}", l, r)),
            },
            BinOp::Eq => Ok(Self::feel_equal(l, r)
                .map(FeelValue::Bool)
                .unwrap_or(FeelValue::Null)),
            BinOp::Neq => Ok(Self::feel_equal(l, r)
                .map(|equal| FeelValue::Bool(!equal))
                .unwrap_or(FeelValue::Null)),
            BinOp::Lt => match Self::compare(l, r) {
                Some(std::cmp::Ordering::Less) => Ok(FeelValue::Bool(true)),
                Some(_) => Ok(FeelValue::Bool(false)),
                None => Ok(FeelValue::Null),
            },
            BinOp::Lte => match Self::compare(l, r) {
                Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal) => {
                    Ok(FeelValue::Bool(true))
                }
                Some(_) => Ok(FeelValue::Bool(false)),
                None => Ok(FeelValue::Null),
            },
            BinOp::Gt => match Self::compare(l, r) {
                Some(std::cmp::Ordering::Greater) => Ok(FeelValue::Bool(true)),
                Some(_) => Ok(FeelValue::Bool(false)),
                None => Ok(FeelValue::Null),
            },
            BinOp::Gte => match Self::compare(l, r) {
                Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal) => {
                    Ok(FeelValue::Bool(true))
                }
                Some(_) => Ok(FeelValue::Bool(false)),
                None => Ok(FeelValue::Null),
            },
            BinOp::In => unreachable!(), // handled in eval_in
        }
    }

    fn ternary_and(left: &FeelValue, right: &FeelValue) -> FeelValue {
        match (left, right) {
            (FeelValue::Bool(false), _) | (_, FeelValue::Bool(false)) => FeelValue::Bool(false),
            (FeelValue::Bool(true), FeelValue::Bool(true)) => FeelValue::Bool(true),
            _ => FeelValue::Null,
        }
    }

    fn feel_equal(left: &FeelValue, right: &FeelValue) -> Option<bool> {
        match (left, right) {
            (FeelValue::Null, FeelValue::Null) => Some(true),
            (FeelValue::Null, _) | (_, FeelValue::Null) => Some(false),
            (FeelValue::Date(_), FeelValue::Date(_))
            | (FeelValue::Time(_), FeelValue::Time(_))
            | (FeelValue::DateTime(_), FeelValue::DateTime(_)) => {
                Self::compare(left, right).map(|ordering| ordering == Ordering::Equal)
            }
            (FeelValue::Bool(_), FeelValue::Bool(_))
            | (FeelValue::Number(_), FeelValue::Number(_))
            | (FeelValue::String(_), FeelValue::String(_))
            | (FeelValue::Duration(_), FeelValue::Duration(_))
            | (FeelValue::List(_), FeelValue::List(_))
            | (FeelValue::Context(_), FeelValue::Context(_))
            | (FeelValue::Range { .. }, FeelValue::Range { .. }) => Some(left == right),
            _ => None,
        }
    }

    fn compare(left: &FeelValue, right: &FeelValue) -> Option<Ordering> {
        match (left, right) {
            (FeelValue::Date(left), FeelValue::Date(right)) => Self::parse_date(left)
                .ok()?
                .partial_cmp(&Self::parse_date(right).ok()?),
            (FeelValue::Time(left), FeelValue::Time(right)) => {
                let left_zoned = DateTime::parse_from_rfc3339(&format!("1970-01-01T{left}"));
                let right_zoned = DateTime::parse_from_rfc3339(&format!("1970-01-01T{right}"));
                match (left_zoned, right_zoned) {
                    (Ok(left), Ok(right)) => left.partial_cmp(&right),
                    (Err(_), Err(_)) => Self::parse_time(left)
                        .ok()?
                        .partial_cmp(&Self::parse_time(right).ok()?),
                    _ => None,
                }
            }
            (FeelValue::DateTime(left), FeelValue::DateTime(right)) => {
                match (
                    DateTime::parse_from_rfc3339(left),
                    DateTime::parse_from_rfc3339(right),
                ) {
                    (Ok(left), Ok(right)) => left.partial_cmp(&right),
                    (Err(_), Err(_)) => NaiveDateTime::parse_from_str(left, "%Y-%m-%dT%H:%M:%S%.f")
                        .ok()?
                        .partial_cmp(
                            &NaiveDateTime::parse_from_str(right, "%Y-%m-%dT%H:%M:%S%.f").ok()?,
                        ),
                    _ => None,
                }
            }
            _ => left.partial_cmp(right),
        }
    }

    fn contains_value(container: &FeelValue, value: &FeelValue) -> Option<bool> {
        match container {
            FeelValue::List(items) => {
                let mut has_null = false;
                for item in items {
                    match Self::feel_equal(value, item) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => has_null = true,
                    }
                }
                if has_null { None } else { Some(false) }
            }
            FeelValue::Range {
                start,
                end,
                start_open,
                end_open,
            } => {
                let lower = if matches!(**start, FeelValue::Null) {
                    Some(true)
                } else {
                    Self::compare(value, start).map(|order| {
                        order == Ordering::Greater || (!start_open && order == Ordering::Equal)
                    })
                };
                let upper = if matches!(**end, FeelValue::Null) {
                    Some(true)
                } else {
                    Self::compare(value, end).map(|order| {
                        order == Ordering::Less || (!end_open && order == Ordering::Equal)
                    })
                };
                match (lower, upper) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                }
            }
            _ => Self::feel_equal(value, container),
        }
    }
}

impl Default for FeelInterpreter {
    fn default() -> Self {
        Self::new()
    }
}
