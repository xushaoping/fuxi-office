//! FEEL (Friendly Enough Expression Language) interpreter for DMN 1.5.

mod ast;
mod builtins;
mod format;
mod interpreter;
mod lexer;
mod parser;
mod value;

pub use ast::{BinOp, FeelExpr, QuantKind, UnOp, UnaryTest};
pub use interpreter::FeelInterpreter;
pub use value::{FeelRenderError, FeelValue};

#[derive(Debug, Clone)]
pub struct CompiledFeel {
    expression: FeelExpr,
}

impl CompiledFeel {
    pub fn expression(&self) -> &FeelExpr {
        &self.expression
    }

    pub fn eval(&self) -> Result<FeelValue, String> {
        FeelInterpreter::new().eval_ast(&self.expression)
    }

    pub fn eval_with(&self, context: &HashMap<String, FeelValue>) -> Result<FeelValue, String> {
        FeelInterpreter::new().eval_expr(&self.expression, context)
    }
}

pub fn compile_feel(input: &str) -> Result<CompiledFeel, String> {
    Ok(CompiledFeel {
        expression: parse_feel(input)?,
    })
}

use lexer::Lexer;
use parser::Parser;
use std::collections::HashMap;

pub fn parse_feel(input: &str) -> Result<FeelExpr, String> {
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize()?;
    Parser::new(tokens).parse()
}

pub fn eval_feel(input: &str) -> Result<FeelValue, String> {
    FeelInterpreter::new().eval(input)
}

pub fn eval_feel_with(input: &str, ctx: HashMap<String, FeelValue>) -> Result<FeelValue, String> {
    FeelInterpreter::with_context(ctx).eval(input)
}

#[cfg(test)]
mod tests;
