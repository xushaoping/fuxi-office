// ============================================================
//  词法分析器
// ============================================================

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Token {
    // 字面量
    Null,
    True,
    False,
    Number(f64),
    String_(String),
    Date(String),     // date("...")
    DateTime(String), // date and time("...")

    // 标识符与关键字
    Name(String),
    If,
    Then,
    Else,
    For,
    In,
    Not,
    And,
    Or,
    Some,
    Every,
    Satisfies,
    Function,
    Return,

    // 运算符
    Add,
    Sub,
    Mul,
    Div,
    Pow, // ** exponentiation
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    DotDot, // ..
    Arrow,  // ->
    Assign, // :
    Comma,
    Dot,
    Pipe, // |

    // 括号
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // DMN 1.5 新增
    At,       // @ 日期/时间前缀
    Between,  // between 关键字
    Instance, // instance 关键字
    Is,       // is 关键字
}

pub(super) struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "null"
            | "true"
            | "false"
            | "if"
            | "then"
            | "else"
            | "for"
            | "in"
            | "not"
            | "and"
            | "or"
            | "some"
            | "every"
            | "satisfies"
            | "function"
            | "return"
            | "between"
            | "instance"
            | "of"
            | "is"
    )
}

impl Lexer {
    pub(super) fn new(input: &str) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        self.pos += 1;
        ch
    }

    fn lookahead(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() || ch == '\n' || ch == '\r' || ch == '\t' {
                self.advance();
            } else if ch == '/' && self.lookahead(1) == Some('/') {
                // 单行注释
                while let Some(c) = self.advance() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    pub(super) fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => break,
                Some(ch) => {
                    let tok = match ch {
                        '(' => {
                            self.advance();
                            Token::LParen
                        }
                        ')' => {
                            self.advance();
                            Token::RParen
                        }
                        '[' => {
                            self.advance();
                            Token::LBracket
                        }
                        ']' => {
                            self.advance();
                            Token::RBracket
                        }
                        '{' => {
                            self.advance();
                            Token::LBrace
                        }
                        '}' => {
                            self.advance();
                            Token::RBrace
                        }
                        ',' => {
                            self.advance();
                            Token::Comma
                        }
                        '.' => {
                            self.advance();
                            if self.peek() == Some('.') {
                                self.advance();
                                Token::DotDot
                            } else {
                                Token::Dot
                            }
                        }
                        '|' => {
                            self.advance();
                            Token::Pipe
                        }
                        '+' => {
                            self.advance();
                            Token::Add
                        }
                        '-' => {
                            self.advance();
                            if self.peek() == Some('>') {
                                self.advance();
                                Token::Arrow
                            } else {
                                Token::Sub
                            }
                        }
                        '*' => {
                            self.advance();
                            if self.peek() == Some('*') {
                                self.advance();
                                Token::Pow
                            } else {
                                Token::Mul
                            }
                        }
                        '/' => {
                            self.advance();
                            Token::Div
                        }
                        '=' => {
                            self.advance();
                            if self.peek() == Some('=') {
                                self.advance();
                            }
                            Token::Eq // DMN 1.5: both = and == are equality
                        }
                        '!' => {
                            self.advance();
                            if self.peek() == Some('=') {
                                self.advance();
                                Token::Neq
                            } else {
                                return Err("Expected '!=', found lone '!'".into());
                            }
                        }
                        '<' => {
                            self.advance();
                            if self.peek() == Some('=') {
                                self.advance();
                                Token::Lte
                            } else {
                                Token::Lt
                            }
                        }
                        '>' => {
                            self.advance();
                            if self.peek() == Some('=') {
                                self.advance();
                                Token::Gte
                            } else {
                                Token::Gt
                            }
                        }
                        ':' => {
                            self.advance();
                            Token::Assign
                        }
                        '@' => {
                            self.advance();
                            Token::At
                        }
                        '"' => self.read_string()?,
                        '\'' => self.read_single_quoted_string()?,
                        _ if ch.is_ascii_digit() => self.read_number()?,
                        _ if ch.is_alphabetic() || ch == '_' => self.read_name()?,
                        _ => return Err(format!("Unexpected character: '{}'", ch)),
                    };
                    tokens.push(tok);
                }
            }
        }
        Ok(tokens)
    }

    fn read_string(&mut self) -> Result<Token, String> {
        self.advance(); // skip opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err("Unterminated string".into()),
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => return Err("Unterminated escape sequence".into()),
                },
                Some(c) => s.push(c),
            }
        }
        // 检查是否是 date("...") 或 date and time("...")
        if s.starts_with("date and time(\"") && s.ends_with("\")") {
            // `date and time("` is 15 bytes; keeping the first year digit is
            // important for values such as `2026-09-05T...`.
            let inner = s[15..s.len() - 2].to_string();
            Ok(Token::DateTime(inner))
        } else if s.starts_with("date(\"") && s.ends_with("\")") {
            let inner = s[6..s.len() - 2].to_string();
            Ok(Token::Date(inner))
        } else {
            Ok(Token::String_(s))
        }
    }

    fn read_single_quoted_string(&mut self) -> Result<Token, String> {
        self.advance(); // skip '
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err("Unterminated string".into()),
                Some('\'') => {
                    // 检查转义: '' 表示 '
                    if self.peek() == Some('\'') {
                        self.advance();
                        s.push('\'');
                    } else {
                        break;
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Ok(Token::String_(s))
    }

    fn read_number(&mut self) -> Result<Token, String> {
        fn parse_number(value: &str) -> Result<Token, String> {
            let number = value
                .parse::<f64>()
                .map_err(|error| format!("Invalid number '{value}': {error}"))?;
            if !number.is_finite() {
                return Err(format!("Invalid non-finite number '{value}'"));
            }
            Ok(Token::Number(number))
        }

        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit()
                || ch == '.'
                || ch == 'e'
                || ch == 'E'
                || (ch == '-' && s.ends_with('e'))
                || (ch == '+' && s.ends_with('e'))
            {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        // If string contains "..", split: "1..5" -> Number(1), then "..5" needs to be handled
        if let Some(pos) = s.find("..") {
            let num_part = &s[..pos];
            parse_number(num_part)?;
            // Rewind by len("..") + 1 (the "..")
            self.pos -= s.len() - pos;
            parse_number(num_part)
        } else {
            parse_number(&s)
        }
    }

    fn read_name(&mut self) -> Result<Token, String> {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' || (ch == '-' && !s.is_empty()) {
                s.push(ch);
                self.advance();
                continue;
            }
            if !ch.is_whitespace() || s.is_empty() {
                break;
            }

            let whitespace_start = self.pos;
            self.skip_whitespace();
            let next_word_start = self.pos;
            while self
                .peek()
                .is_some_and(|next| next.is_alphanumeric() || next == '_')
            {
                self.advance();
            }
            let next_word = self.chars[next_word_start..self.pos]
                .iter()
                .collect::<String>();
            let instance_of = s.eq_ignore_ascii_case("instance") && next_word == "of";
            let name_word = next_word == "of" && !s.eq_ignore_ascii_case("instance");
            if is_keyword(&s)
                || next_word.is_empty()
                || (is_keyword(&next_word) && !name_word)
                || instance_of
            {
                self.pos = whitespace_start;
                break;
            }
            s.push(' ');
            s.push_str(&next_word);
        }
        // Only continue past a space if the current word is a prefix of a multi-word keyword
        // Currently: "date" can be followed by " and time" -> "date and time"
        if s.to_lowercase() == "date" && self.peek() == Some(' ') {
            let saved_pos = self.pos;
            // Try reading " and time"
            let mut full = s.clone();
            full.push(' ');
            self.advance(); // consume the space
            while let Some(ch) = self.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    full.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            if full.to_lowercase() == "date and time" {
                return Ok(Token::DateTime(full));
            }
            // Not "date and time", treat "date" as a standalone name and backtrack
            self.pos = saved_pos;
        }
        // "instance" can be followed by " of" -> "instance of" (handled by parser for now)
        if s.to_lowercase() == "instance" && self.peek() == Some(' ') {
            // Just return Instance token, parser will look for "of"
        }

        match s.to_lowercase().as_str() {
            "null" => Ok(Token::Null),
            "true" => Ok(Token::True),
            "false" => Ok(Token::False),
            "if" => Ok(Token::If),
            "then" => Ok(Token::Then),
            "else" => Ok(Token::Else),
            "for" => Ok(Token::For),
            "in" => Ok(Token::In),
            "not" => Ok(Token::Not),
            "and" => Ok(Token::And),
            "or" => Ok(Token::Or),
            "some" => Ok(Token::Some),
            "every" => Ok(Token::Every),
            "satisfies" => Ok(Token::Satisfies),
            "function" => Ok(Token::Function),
            "return" => Ok(Token::Return),
            "between" => Ok(Token::Between),
            "instance" => Ok(Token::Instance),
            "is" => Ok(Token::Is),
            _ => Ok(Token::Name(s)),
        }
    }
}
