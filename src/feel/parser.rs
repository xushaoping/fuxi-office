use super::ast::*;
use super::lexer::Token;

// ============================================================
//  递归下降解析器
// ============================================================

pub(super) struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        match self.advance() {
            Some(t) if t == *expected => Ok(()),
            Some(t) => Err(format!("Expected {:?}, got {:?}", expected, t)),
            None => Err(format!("Expected {:?}, got EOF", expected)),
        }
    }

    fn at(&self, tok: &Token) -> bool {
        self.peek() == Some(tok)
    }

    /// Known multi-word function names in FEEL
    const MULTI_WORD_NAMES: &'static [&'static str] = &[
        "string length",
        "upper case",
        "lower case",
        "starts with",
        "ends with",
        "distinct values",
        "day of week",
        "list contains",
        "index of",
        "get entries",
        "get value",
        "is defined",
        "type of",
        "date and time",
        "instance of",
        "substring before",
        "substring after",
        "day of year",
        "month of year",
        "week of year",
        "round up",
        "round down",
        "round half up",
        "round half down",
        "years and months duration",
        "list replace",
        "string join",
        "context put",
        "context merge",
        "met by",
        "overlaps before",
        "overlaps after",
        "finished by",
        "started by",
        "insert before",
        "sublist",
        "remove",
        "union",
        "product",
        "median",
        "stddev",
        "all",
        "any",
        "before",
        "after",
        "meets",
        "overlaps",
        "includes",
        "during",
        "starts",
        "coincides",
        "finishes",
        "decimal",
        "odd",
        "even",
        "context",
        "range",
        "date",
        "time",
        "duration",
        "is",
        "not",
    ];

    /// Try to combine the current Name with following Name tokens to form a multi-word name.
    /// Also handles "date and time" where "and" is a keyword token.
    /// Backtracks if no multi-word match is found.
    fn try_read_multi_word_name(&mut self, first: &str) -> String {
        let saved_pos = self.pos;
        let mut words = vec![first.to_string()];

        loop {
            if let Some(Token::Name(next)) = self.peek().cloned() {
                words.push(next.clone());
                self.advance();
            } else if let Some(Token::And) = self.peek() {
                // Special case: "and" as part of "date and time"
                words.push("and".to_string());
                self.advance();
            } else {
                break;
            }
            let candidate = words.join(" ");
            let is_prefix = Self::MULTI_WORD_NAMES
                .iter()
                .any(|&mw| mw.starts_with(&candidate));
            if !is_prefix {
                // Remove the last word and backtrack
                words.pop();
                self.pos -= 1;
                break;
            }
            let is_exact = Self::MULTI_WORD_NAMES.iter().any(|&mw| *mw == candidate);
            if is_exact {
                // Found a multi-word name match, continue in case there's a longer one
            }
        }

        let result = words.join(" ");
        if Self::MULTI_WORD_NAMES.iter().any(|&mw| *mw == result) {
            result
        } else {
            self.pos = saved_pos;
            first.to_string()
        }
    }

    pub(super) fn parse(&mut self) -> Result<FeelExpr, String> {
        let expr = self.parse_or()?;
        if self.pos < self.tokens.len() {
            return Err(format!(
                "Unexpected token after expression: {:?}",
                self.peek()
            ));
        }
        Ok(expr)
    }

    // or 表达式 (短路)
    fn parse_or(&mut self) -> Result<FeelExpr, String> {
        let mut exprs = vec![self.parse_and()?];
        while self.at(&Token::Or) {
            self.advance();
            exprs.push(self.parse_and()?);
        }
        if exprs.len() == 1 {
            Ok(exprs.pop().unwrap())
        } else {
            Ok(FeelExpr::Or(exprs))
        }
    }

    // and 表达式 (短路)
    fn parse_and(&mut self) -> Result<FeelExpr, String> {
        let mut exprs = vec![self.parse_comparison()?];
        while self.at(&Token::And) {
            self.advance();
            exprs.push(self.parse_comparison()?);
        }
        if exprs.len() == 1 {
            Ok(exprs.pop().unwrap())
        } else {
            Ok(FeelExpr::And(exprs))
        }
    }

    // 比较表达式: =, !=, <, <=, >, >=, in, not in, between, instance of, is, is not
    fn parse_comparison(&mut self) -> Result<FeelExpr, String> {
        let left = self.parse_addition()?;

        // 处理 not in
        if self.at(&Token::Not) {
            let saved = self.pos;
            self.advance();
            if self.at(&Token::In) {
                self.advance();
                let right = self.parse_addition()?;
                return Ok(FeelExpr::Not(Box::new(FeelExpr::Binary(
                    Box::new(left),
                    BinOp::In,
                    Box::new(right),
                ))));
            } else {
                self.pos = saved;
            }
        }

        // between expr and expr
        if self.at(&Token::Between) {
            self.advance();
            let low = self.parse_addition()?;
            self.expect(&Token::And)?;
            let high = self.parse_addition()?;
            return Ok(FeelExpr::Between {
                value: Box::new(left),
                low: Box::new(low),
                high: Box::new(high),
            });
        }

        // instance of Type
        // Note: "instance" is already returned as Token::Instance keyword
        if self.at(&Token::Instance) {
            self.advance();
            // check for "of"
            if let Some(Token::Name(of)) = self.peek().cloned()
                && of.to_lowercase() == "of"
            {
                self.advance();
                // Type name can be a simple Name or "date and time" (DateTime)
                let type_name = match self.advance() {
                    Some(Token::Name(n)) => n,
                    Some(Token::DateTime(dt)) => dt,
                    t => {
                        return Err(format!(
                            "Expected type name after 'instance of', got {:?}",
                            t
                        ));
                    }
                };
                return Ok(FeelExpr::InstanceOf {
                    value: Box::new(left),
                    type_name,
                });
            }
            return Err("Expected 'of' after 'instance'".into());
        }

        // is / is not
        if self.at(&Token::Is) {
            self.advance();
            if self.at(&Token::Not) {
                self.advance();
                let right = self.parse_addition()?;
                return Ok(FeelExpr::IsNot(Box::new(left), Box::new(right)));
            }
            let right = self.parse_addition()?;
            return Ok(FeelExpr::Is(Box::new(left), Box::new(right)));
        }

        let op = match self.peek() {
            Some(Token::Eq) => BinOp::Eq,
            Some(Token::Neq) => BinOp::Neq,
            Some(Token::Lt) => BinOp::Lt,
            Some(Token::Lte) => BinOp::Lte,
            Some(Token::Gt) => BinOp::Gt,
            Some(Token::Gte) => BinOp::Gte,
            Some(Token::In) => BinOp::In,
            _ => return Ok(left),
        };
        self.advance();

        // in with positive unary tests: x in (< 10, >= 5)
        if matches!(op, BinOp::In) && self.at(&Token::LParen) {
            return self.parse_in_unary_tests(left);
        }

        let right = self.parse_addition()?;
        Ok(FeelExpr::Binary(Box::new(left), op, Box::new(right)))
    }

    // Parse "in" with unary tests: x in (< 10, >= 5, [1..10])
    fn parse_in_unary_tests(&mut self, left: FeelExpr) -> Result<FeelExpr, String> {
        self.expect(&Token::LParen)?;
        let mut tests = Vec::new();
        loop {
            let test = match self.peek().cloned() {
                Some(Token::Lt) => {
                    self.advance();
                    UnaryTest::LessThan(Box::new(self.parse_addition()?))
                }
                Some(Token::Lte) => {
                    self.advance();
                    UnaryTest::LessOrEqual(Box::new(self.parse_addition()?))
                }
                Some(Token::Gt) => {
                    self.advance();
                    UnaryTest::GreaterThan(Box::new(self.parse_addition()?))
                }
                Some(Token::Gte) => {
                    self.advance();
                    UnaryTest::GreaterOrEqual(Box::new(self.parse_addition()?))
                }
                Some(Token::LBracket) | Some(Token::LParen) => {
                    // Could be a range literal
                    let expr = self.parse_addition()?;
                    UnaryTest::Range(Box::new(expr))
                }
                _ => {
                    let expr = self.parse_addition()?;
                    UnaryTest::Value(Box::new(expr))
                }
            };
            tests.push(test);
            if !self.at(&Token::Comma) {
                break;
            }
            self.advance();
        }
        self.expect(&Token::RParen)?;
        Ok(FeelExpr::InUnaryTests {
            value: Box::new(left),
            tests,
        })
    }

    // 加减
    fn parse_addition(&mut self) -> Result<FeelExpr, String> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.peek() {
                Some(Token::Add) => BinOp::Add,
                Some(Token::Sub) => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = FeelExpr::Binary(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    // 乘除
    fn parse_multiplication(&mut self) -> Result<FeelExpr, String> {
        let mut left = self.parse_power()?;
        loop {
            let op = match self.peek() {
                Some(Token::Mul) => BinOp::Mul,
                Some(Token::Div) => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_power()?;
            left = FeelExpr::Binary(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    // 幂运算 ** (right-associative)
    fn parse_power(&mut self) -> Result<FeelExpr, String> {
        let base = self.parse_unary()?;
        if self.at(&Token::Pow) {
            self.advance();
            let exp = self.parse_power()?; // right-associative recursion
            Ok(FeelExpr::Binary(Box::new(base), BinOp::Pow, Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    // 一元: -, not
    fn parse_unary(&mut self) -> Result<FeelExpr, String> {
        if self.at(&Token::Sub) {
            self.advance();
            let expr = self.parse_unary()?;
            Ok(FeelExpr::Unary(UnOp::Minus, Box::new(expr)))
        } else if self.at(&Token::Not) {
            self.advance();
            let expr = self.parse_unary()?;
            Ok(FeelExpr::Not(Box::new(expr)))
        } else {
            self.parse_postfix()
        }
    }

    // 后缀: ., [], ()
    fn parse_postfix(&mut self) -> Result<FeelExpr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.at(&Token::Dot) {
                self.advance();
                if let Some(Token::Name(name)) = self.peek().cloned() {
                    self.advance();
                    expr = FeelExpr::Path(Box::new(expr), name);
                } else {
                    return Err("Expected property name after '.'".into());
                }
            } else if self.at(&Token::LBracket) {
                self.advance();
                let index = self.parse_or()?;
                self.expect(&Token::RBracket)?;
                expr = FeelExpr::Filter(Box::new(expr), Box::new(index));
            } else if self.at(&Token::LParen) {
                // 函数调用 (positional or named parameters)
                self.advance();
                // Check for named parameters: name: expr
                let is_named = if !self.at(&Token::RParen) {
                    if self.pos + 2 < self.tokens.len() {
                        matches!(self.tokens.get(self.pos), Some(Token::Name(_)))
                            && matches!(self.tokens.get(self.pos + 1), Some(Token::Assign))
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_named {
                    // Named parameters
                    let mut args = Vec::new();
                    if !self.at(&Token::RParen) {
                        let key = match self.advance() {
                            Some(Token::Name(n)) => n,
                            t => return Err(format!("Expected parameter name, got {:?}", t)),
                        };
                        self.expect(&Token::Assign)?;
                        let val = self.parse_or()?;
                        args.push((key, val));
                        while self.at(&Token::Comma) {
                            self.advance();
                            let key = match self.advance() {
                                Some(Token::Name(n)) => n,
                                t => return Err(format!("Expected parameter name, got {:?}", t)),
                            };
                            self.expect(&Token::Assign)?;
                            let val = self.parse_or()?;
                            args.push((key, val));
                        }
                    }
                    self.expect(&Token::RParen)?;
                    if let FeelExpr::Name(name) = expr {
                        expr = FeelExpr::CallNamed { name, args };
                    } else {
                        return Err("Expected function name before '('".into());
                    }
                } else {
                    // Positional parameters
                    let mut args = Vec::new();
                    if !self.at(&Token::RParen) {
                        args.push(self.parse_or()?);
                        while self.at(&Token::Comma) {
                            self.advance();
                            args.push(self.parse_or()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    if let FeelExpr::Name(name) = expr {
                        expr = FeelExpr::Call(name, args);
                    } else {
                        return Err("Expected function name before '('".into());
                    }
                }
            } else {
                break;
            }
        }
        Ok(expr)
    }

    // 主表达式
    fn parse_primary(&mut self) -> Result<FeelExpr, String> {
        match self.peek().cloned() {
            Some(Token::Null) => {
                self.advance();
                Ok(FeelExpr::Null)
            }
            Some(Token::True) => {
                self.advance();
                Ok(FeelExpr::Bool(true))
            }
            Some(Token::False) => {
                self.advance();
                Ok(FeelExpr::Bool(false))
            }
            Some(Token::Number(n)) => {
                self.advance();
                Ok(FeelExpr::Number(n))
            }
            Some(Token::String_(s)) => {
                self.advance();
                Ok(FeelExpr::String(s))
            }
            Some(Token::Date(d)) => {
                self.advance();
                Ok(FeelExpr::DateLit(d))
            }
            Some(Token::DateTime(dt)) => {
                self.advance();
                Ok(FeelExpr::DateTimeLit(dt))
            }
            Some(Token::Name(name)) => {
                self.advance();
                let full_name = self.try_read_multi_word_name(&name);
                Ok(FeelExpr::Name(full_name))
            }

            Some(Token::LParen) => {
                // Could be: grouped expression OR range literal like (1..10]
                let saved_pos = self.pos;
                self.advance(); // consume (

                // Try parsing as range: (expr .. expr ]  or (expr .. expr )
                let start_expr = self.parse_or()?;
                if self.at(&Token::DotDot) {
                    self.advance(); // consume ..
                    let end_expr = self.parse_or()?;
                    let end_open = match self.peek() {
                        Some(Token::RParen) => {
                            self.advance();
                            true
                        }
                        Some(Token::RBracket) => {
                            self.advance();
                            false
                        }
                        _ => return Err("Expected ) or ] after range end".into()),
                    };
                    return Ok(FeelExpr::Interval {
                        start: Box::new(start_expr),
                        end: Box::new(end_expr),
                        start_open: true, // ( means open start
                        end_open,
                    });
                }

                // Not a range, backtrack and parse as grouped expression
                self.pos = saved_pos;
                self.advance(); // consume ( again
                let expr = self.parse_or()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }

            Some(Token::LBracket) => self.parse_list_or_for(),

            Some(Token::LBrace) => self.parse_context_or_interval(),

            Some(Token::If) => self.parse_if(),

            Some(Token::For) => self.parse_for(),

            Some(Token::Some) | Some(Token::Every) => self.parse_quantified(),

            Some(Token::Function) => self.parse_function_def(),

            Some(Token::At) => {
                self.advance();
                // @ followed by string literal: @"2024-01-01"
                match self.advance() {
                    Some(Token::String_(s)) => Ok(FeelExpr::AtLiteral(s)),
                    t => Err(format!("Expected string after '@', got {:?}", t)),
                }
            }

            Some(t) => Err(format!("Unexpected token in primary: {:?}", t)),
            None => Err("Unexpected end of input".into()),
        }
    }

    // 列表 [1, 2, 3] 或范围字面量 [1..10] 或列表推导 [for x in list: expr]
    fn parse_list_or_for(&mut self) -> Result<FeelExpr, String> {
        self.expect(&Token::LBracket)?;
        if self.at(&Token::For) {
            self.advance();
            let var = match self.advance() {
                Some(Token::Name(n)) => n,
                t => return Err(format!("Expected variable name after 'for', got {:?}", t)),
            };
            self.expect(&Token::In)?;
            let iter = self.parse_or()?;
            self.expect(&Token::Assign)?;
            let body = self.parse_or()?;
            self.expect(&Token::RBracket)?;
            Ok(FeelExpr::ForExpr {
                var,
                iter: Box::new(iter),
                body: Box::new(body),
            })
        } else if self.at(&Token::RBracket) {
            // Empty list
            self.advance();
            Ok(FeelExpr::List(Vec::new()))
        } else {
            // Try parsing first item, check if it's a range: [expr .. expr]
            let first = self.parse_or()?;
            if self.at(&Token::DotDot) {
                self.advance(); // consume ..
                let end = self.parse_or()?;
                let end_open = if self.at(&Token::RParen) {
                    self.advance();
                    true
                } else {
                    self.expect(&Token::RBracket)?;
                    false
                };
                Ok(FeelExpr::Interval {
                    start: Box::new(first),
                    end: Box::new(end),
                    start_open: false, // [ means closed start
                    end_open,
                })
            } else {
                // It's a list
                let mut items = vec![first];
                while self.at(&Token::Comma) {
                    self.advance();
                    items.push(self.parse_or()?);
                }
                self.expect(&Token::RBracket)?;
                Ok(FeelExpr::List(items))
            }
        }
    }

    // 上下文 { k: v } 或区间 (a..b)
    fn parse_context_or_interval(&mut self) -> Result<FeelExpr, String> {
        self.expect(&Token::LBrace)?;
        let mut entries = Vec::new();
        if !self.at(&Token::RBrace) {
            loop {
                let key = match self.advance() {
                    Some(Token::Name(n)) => n,
                    Some(Token::String_(s)) => s,
                    t => return Err(format!("Expected key in context, got {:?}", t)),
                };
                self.expect(&Token::Assign)?;
                let value = self.parse_or()?;
                entries.push((key, value));
                if !self.at(&Token::Comma) {
                    break;
                }
                self.advance();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(FeelExpr::Context(entries))
    }

    // if(cond, then, else) or if cond then a else b (DMN 1.5)
    fn parse_if(&mut self) -> Result<FeelExpr, String> {
        self.expect(&Token::If)?;
        if self.at(&Token::LParen) {
            // Old function-call style: if(cond, then, else)
            self.advance();
            let cond = self.parse_or()?;
            self.expect(&Token::Comma)?;
            let then_ = self.parse_or()?;
            self.expect(&Token::Comma)?;
            let else_ = self.parse_or()?;
            self.expect(&Token::RParen)?;
            Ok(FeelExpr::IfExpr {
                cond: Box::new(cond),
                then_: Box::new(then_),
                else_: Box::new(else_),
            })
        } else {
            // DMN 1.5 keyword style: if cond then a else b
            let cond = self.parse_or()?;
            self.expect(&Token::Then)?;
            let then_ = self.parse_or()?;
            self.expect(&Token::Else)?;
            let else_ = self.parse_or()?;
            Ok(FeelExpr::IfThenElse {
                cond: Box::new(cond),
                then_: Box::new(then_),
                else_: Box::new(else_),
            })
        }
    }

    // for x in list return expr  (独立for表达式，不在[]内)
    fn parse_for(&mut self) -> Result<FeelExpr, String> {
        self.expect(&Token::For)?;
        let var = match self.advance() {
            Some(Token::Name(n)) => n,
            t => return Err(format!("Expected variable name after 'for', got {:?}", t)),
        };
        self.expect(&Token::In)?;
        let iter = self.parse_or()?;
        self.expect(&Token::Return)?;
        let body = self.parse_or()?;
        Ok(FeelExpr::ForExpr {
            var,
            iter: Box::new(iter),
            body: Box::new(body),
        })
    }

    // some x in list satisfies expr
    // every x in list satisfies expr
    fn parse_quantified(&mut self) -> Result<FeelExpr, String> {
        let kind = match self.advance() {
            Some(Token::Some) => QuantKind::Some,
            Some(Token::Every) => QuantKind::Every,
            t => return Err(format!("Expected 'some' or 'every', got {:?}", t)),
        };
        let var = match self.advance() {
            Some(Token::Name(n)) => n,
            t => return Err(format!("Expected variable name, got {:?}", t)),
        };
        self.expect(&Token::In)?;
        let iter = self.parse_or()?;
        self.expect(&Token::Satisfies)?;
        let satisfies = self.parse_or()?;
        Ok(FeelExpr::Quantified {
            kind,
            var,
            iter: Box::new(iter),
            satisfies: Box::new(satisfies),
        })
    }

    // function(x, y) -> expr
    fn parse_function_def(&mut self) -> Result<FeelExpr, String> {
        self.expect(&Token::Function)?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if !self.at(&Token::RParen) {
            if let Some(Token::Name(n)) = self.advance() {
                params.push(n);
            }
            while self.at(&Token::Comma) {
                self.advance();
                if let Some(Token::Name(n)) = self.advance() {
                    params.push(n);
                }
            }
        }
        self.expect(&Token::RParen)?;
        self.expect(&Token::Arrow)?;
        let body = self.parse_or()?;
        Ok(FeelExpr::FunctionDef(params, Box::new(body)))
    }
}
