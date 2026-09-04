use super::ast::{BinOp, FeelExpr, QuantKind, UnOp, UnaryTest};
use std::fmt;

const OR: u8 = 1;
const AND: u8 = 2;
const COMPARISON: u8 = 3;
const ADDITION: u8 = 4;
const MULTIPLICATION: u8 = 5;
const POWER: u8 = 6;
const UNARY: u8 = 7;
const POSTFIX: u8 = 8;
const PRIMARY: u8 = 9;

impl FeelExpr {
    pub fn to_feel_string(&self) -> String {
        self.to_string()
    }

    fn precedence(&self) -> u8 {
        match self {
            Self::Or(_) => OR,
            Self::And(_) => AND,
            Self::Binary(_, op, _) => match op {
                BinOp::Eq
                | BinOp::Neq
                | BinOp::Lt
                | BinOp::Lte
                | BinOp::Gt
                | BinOp::Gte
                | BinOp::In => COMPARISON,
                BinOp::Add | BinOp::Sub => ADDITION,
                BinOp::Mul | BinOp::Div => MULTIPLICATION,
                BinOp::Pow => POWER,
            },
            Self::Between { .. }
            | Self::InstanceOf { .. }
            | Self::Is(_, _)
            | Self::IsNot(_, _)
            | Self::InUnaryTests { .. } => COMPARISON,
            Self::Unary(_, _) | Self::Not(_) => UNARY,
            Self::Path(_, _)
            | Self::Filter(_, _)
            | Self::Index(_, _)
            | Self::Call(_, _)
            | Self::CallNamed { .. } => POSTFIX,
            _ => PRIMARY,
        }
    }

    fn render(&self, f: &mut fmt::Formatter<'_>, parent: u8) -> fmt::Result {
        let precedence = self.precedence();
        let wrap = precedence < parent;
        if wrap {
            write!(f, "(")?;
        }
        match self {
            Self::Null => write!(f, "null")?,
            Self::Bool(value) => write!(f, "{value}")?,
            Self::Number(value) => write!(f, "{value}")?,
            Self::String(value) => write!(f, "\"{}\"", escape(value))?,
            Self::DateLit(value) => write!(f, "date(\"{}\")", escape(value))?,
            Self::DateTimeLit(value) => write!(f, "date and time(\"{}\")", escape(value))?,
            Self::Name(name) => write!(f, "{name}")?,
            Self::QualifiedName(parts) => write!(f, "{}", parts.join("."))?,
            Self::Binary(left, op, right) => {
                left.render(f, precedence)?;
                write!(f, " {} ", binary_operator(*op))?;
                right.render(
                    f,
                    precedence + u8::from(matches!(op, BinOp::Sub | BinOp::Div)),
                )?;
            }
            Self::Unary(UnOp::Minus, value) => {
                write!(f, "-")?;
                value.render(f, precedence)?;
            }
            Self::Unary(UnOp::Neg, value) => value.render(f, precedence)?,
            Self::Not(value) => {
                write!(f, "not ")?;
                value.render(f, precedence)?;
            }
            Self::And(values) => render_joined(f, values, " and ", precedence)?,
            Self::Or(values) => render_joined(f, values, " or ", precedence)?,
            Self::Interval {
                start,
                end,
                start_open,
                end_open,
            } => {
                write!(f, "{}", if *start_open { "(" } else { "[" })?;
                start.render(f, OR)?;
                write!(f, "..")?;
                end.render(f, OR)?;
                write!(f, "{}", if *end_open { ")" } else { "]" })?;
            }
            Self::List(values) => {
                write!(f, "[")?;
                render_joined(f, values, ", ", OR)?;
                write!(f, "]")?;
            }
            Self::ForExpr { var, iter, body } => {
                write!(f, "[for {var} in ")?;
                iter.render(f, OR)?;
                write!(f, ": ")?;
                body.render(f, OR)?;
                write!(f, "]")?;
            }
            Self::Quantified {
                kind,
                var,
                iter,
                satisfies,
            } => {
                write!(
                    f,
                    "{} {var} in ",
                    match kind {
                        QuantKind::Some => "some",
                        QuantKind::Every => "every",
                    }
                )?;
                iter.render(f, OR)?;
                write!(f, " satisfies ")?;
                satisfies.render(f, OR)?;
            }
            Self::IfExpr { cond, then_, else_ } | Self::IfThenElse { cond, then_, else_ } => {
                write!(f, "if ")?;
                cond.render(f, OR)?;
                write!(f, " then ")?;
                then_.render(f, OR)?;
                write!(f, " else ")?;
                else_.render(f, OR)?;
            }
            Self::Call(name, args) => {
                write!(f, "{name}(")?;
                render_joined(f, args, ", ", OR)?;
                write!(f, ")")?;
            }
            Self::CallNamed { name, args } => {
                write!(f, "{name}(")?;
                for (index, (parameter, value)) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{parameter}: ")?;
                    value.render(f, OR)?;
                }
                write!(f, ")")?;
            }
            Self::Context(entries) => {
                write!(f, "{{")?;
                for (index, (name, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{name}: ")?;
                    value.render(f, OR)?;
                }
                write!(f, "}}")?;
            }
            Self::Filter(base, index) | Self::Index(base, index) => {
                base.render(f, precedence)?;
                write!(f, "[")?;
                index.render(f, OR)?;
                write!(f, "]")?;
            }
            Self::Path(base, property) => {
                base.render(f, precedence)?;
                write!(f, ".{property}")?;
            }
            Self::FunctionDef(parameters, body) => {
                write!(f, "function(")?;
                write!(f, "{}", parameters.join(", "))?;
                write!(f, ") -> ")?;
                body.render(f, OR)?;
            }
            Self::Between { value, low, high } => {
                value.render(f, precedence)?;
                write!(f, " between ")?;
                low.render(f, ADDITION)?;
                write!(f, " and ")?;
                high.render(f, ADDITION)?;
            }
            Self::InstanceOf { value, type_name } => {
                value.render(f, precedence)?;
                write!(f, " instance of {type_name}")?;
            }
            Self::AtLiteral(value) => write!(f, "@\"{}\"", escape(value))?,
            Self::Is(left, right) => {
                left.render(f, precedence)?;
                write!(f, " is ")?;
                right.render(f, precedence)?;
            }
            Self::IsNot(left, right) => {
                left.render(f, precedence)?;
                write!(f, " is not ")?;
                right.render(f, precedence)?;
            }
            Self::InUnaryTests { value, tests } => {
                value.render(f, precedence)?;
                write!(f, " in (")?;
                for (index, test) in tests.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{test}")?;
                }
                write!(f, ")")?;
            }
        }
        if wrap {
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl fmt::Display for FeelExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.render(f, OR)
    }
}

impl fmt::Display for UnaryTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LessThan(value) => write!(f, "< {value}"),
            Self::LessOrEqual(value) => write!(f, "<= {value}"),
            Self::GreaterThan(value) => write!(f, "> {value}"),
            Self::GreaterOrEqual(value) => write!(f, ">= {value}"),
            Self::Range(value) | Self::Value(value) => write!(f, "{value}"),
        }
    }
}

fn render_joined(
    f: &mut fmt::Formatter<'_>,
    values: &[FeelExpr],
    separator: &str,
    precedence: u8,
) -> fmt::Result {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            write!(f, "{separator}")?;
        }
        value.render(f, precedence)?;
    }
    Ok(())
}

fn binary_operator(operator: BinOp) -> &'static str {
    match operator {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "**",
        BinOp::Eq => "=",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::In => "in",
    }
}

pub(crate) fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
