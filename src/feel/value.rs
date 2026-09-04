use super::ast::FeelExpr;
use std::collections::HashMap;
use std::fmt;

// ============================================================
//  运行时值
// ============================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeelRenderError {
    NonFiniteNumber,
}

impl fmt::Display for FeelRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteNumber => write!(f, "FEEL cannot serialize a non-finite number"),
        }
    }
}

impl std::error::Error for FeelRenderError {}

#[derive(Debug, Clone)]
pub enum FeelValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Date(String),
    Time(String),
    DateTime(String),
    Duration(String),
    List(Vec<FeelValue>),
    Context(HashMap<String, FeelValue>),
    Range {
        start: Box<FeelValue>,
        end: Box<FeelValue>,
        start_open: bool,
        end_open: bool,
    },
    Function(Vec<String>, FeelExpr),
}

impl FeelValue {
    pub fn to_feel_string(&self) -> Result<String, FeelRenderError> {
        match self {
            Self::Null => Ok("null".into()),
            Self::Bool(value) => Ok(value.to_string()),
            Self::Number(value) if value.is_finite() => Ok(value.to_string()),
            Self::Number(_) => Err(FeelRenderError::NonFiniteNumber),
            Self::String(value) => Ok(format!("\"{}\"", super::format::escape(value))),
            Self::Date(value) => Ok(format!("date(\"{}\")", super::format::escape(value))),
            Self::Time(value) => Ok(format!("time(\"{}\")", super::format::escape(value))),
            Self::DateTime(value) => Ok(format!(
                "date and time(\"{}\")",
                super::format::escape(value)
            )),
            Self::Duration(value) => Ok(format!("duration(\"{}\")", super::format::escape(value))),
            Self::List(values) => values
                .iter()
                .map(Self::to_feel_string)
                .collect::<Result<Vec<_>, _>>()
                .map(|values| format!("[{}]", values.join(", "))),
            Self::Context(values) => {
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| *key);
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        value
                            .to_feel_string()
                            .map(|value| format!("{key}: {value}"))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(|entries| format!("{{{}}}", entries.join(", ")))
            }
            Self::Range {
                start,
                end,
                start_open,
                end_open,
            } => Ok(format!(
                "{}{}..{}{}",
                if *start_open { "(" } else { "[" },
                start.to_feel_string()?,
                end.to_feel_string()?,
                if *end_open { ")" } else { "]" }
            )),
            Self::Function(parameters, body) => {
                Ok(format!("function({}) -> {body}", parameters.join(", ")))
            }
        }
    }

    pub(crate) fn to_f64(&self) -> Option<f64> {
        match self {
            FeelValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn to_bool(&self) -> Option<bool> {
        match self {
            FeelValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl PartialEq for FeelValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FeelValue::Null, FeelValue::Null) => true,
            (FeelValue::Bool(a), FeelValue::Bool(b)) => a == b,
            (FeelValue::Number(a), FeelValue::Number(b)) => (a - b).abs() < 1e-12,
            (FeelValue::String(a), FeelValue::String(b)) => a == b,
            (FeelValue::List(a), FeelValue::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
            }
            (FeelValue::Date(a), FeelValue::Date(b)) => a == b,
            (FeelValue::Time(a), FeelValue::Time(b)) => a == b,
            (FeelValue::DateTime(a), FeelValue::DateTime(b)) => a == b,
            (FeelValue::Duration(a), FeelValue::Duration(b)) => a == b,
            (FeelValue::Context(a), FeelValue::Context(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k) == Some(v))
            }
            (
                FeelValue::Range {
                    start: s1,
                    end: e1,
                    start_open: so1,
                    end_open: eo1,
                },
                FeelValue::Range {
                    start: s2,
                    end: e2,
                    start_open: so2,
                    end_open: eo2,
                },
            ) => so1 == so2 && eo1 == eo2 && *s1 == *s2 && *e1 == *e2,
            _ => false,
        }
    }
}

impl PartialOrd for FeelValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (FeelValue::Number(a), FeelValue::Number(b)) => a.partial_cmp(b),
            (FeelValue::String(a), FeelValue::String(b)) => a.partial_cmp(b),
            (FeelValue::Bool(a), FeelValue::Bool(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl fmt::Display for FeelValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeelValue::Null => write!(f, "null"),
            FeelValue::Bool(b) => write!(f, "{}", b),
            FeelValue::Number(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{:.0}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            FeelValue::String(s) => write!(f, "\"{}\"", s),
            FeelValue::Date(d) => write!(f, "@\"{}\"", d),
            FeelValue::Time(t) => write!(f, "@\"{}\"", t),
            FeelValue::DateTime(dt) => write!(f, "@\"{}\"", dt),
            FeelValue::Duration(d) => write!(f, "duration(\"{}\")", d),
            FeelValue::List(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            FeelValue::Context(map) => {
                write!(f, "{{")?;
                let mut first = true;
                for (k, v) in map {
                    if !first {
                        write!(f, ", ")?;
                    }
                    first = false;
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            FeelValue::Range {
                start,
                end,
                start_open,
                end_open,
            } => {
                write!(
                    f,
                    "{}{}..{}{}",
                    if *start_open { "(" } else { "[" },
                    start,
                    end,
                    if *end_open { ")" } else { "]" }
                )
            }
            FeelValue::Function(_, _) => write!(f, "<function>"),
        }
    }
}
