//! FEEL (Friendly Enough Expression Language) interpreter for DMN 1.5.

mod ast;
mod builtins;
mod format;
mod interpreter;
mod lexer;
mod parser;
mod value;

pub use ast::{FeelExpr, UnaryTest};
pub use interpreter::FeelInterpreter;
pub use value::FeelValue;

// 编译态 FEEL API：仅由 tests.rs（compile_feel 用例）直接使用；产品执行链路以
// parse_feel + eval_expr 走解释器路径，非测试构建视为预留 API。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct CompiledFeel {
    expression: FeelExpr,
}

#[cfg_attr(not(test), allow(dead_code))]
impl CompiledFeel {
    pub fn expression(&self) -> &FeelExpr {
        &self.expression
    }

    #[allow(dead_code)] // eval 无调用者（含测试），与 eval_with 对称保留
    pub fn eval(&self) -> Result<FeelValue, String> {
        FeelInterpreter::new().eval_ast(&self.expression)
    }

    pub fn eval_with(&self, context: &HashMap<String, FeelValue>) -> Result<FeelValue, String> {
        FeelInterpreter::new().eval_expr(&self.expression, context)
    }
}

#[cfg_attr(not(test), allow(dead_code))] // 测试专用编译 API，产品直接走 parse_feel
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

#[cfg_attr(not(test), allow(dead_code))] // 测试专用求值 API
pub fn eval_feel(input: &str) -> Result<FeelValue, String> {
    FeelInterpreter::new().eval(input)
}

#[cfg_attr(not(test), allow(dead_code))] // 测试专用求值 API
pub fn eval_feel_with(input: &str, ctx: HashMap<String, FeelValue>) -> Result<FeelValue, String> {
    FeelInterpreter::with_context(ctx).eval(input)
}

#[cfg(test)]
mod tests;
