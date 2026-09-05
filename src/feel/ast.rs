// ============================================================
//  AST 节点定义
// ============================================================

#[derive(Debug, Clone)]
pub enum FeelExpr {
    // 字面量
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    DateLit(String),
    DateTimeLit(String),

    // 标识符与变量
    Name(String),
    // 点分链 a.b.c：当前 parser 以 Path 链产出，保留节点供解释/格式化/降级转换匹配
    #[allow(dead_code)]
    QualifiedName(Vec<String>),

    // 二元运算
    Binary(Box<FeelExpr>, BinOp, Box<FeelExpr>),

    // 一元运算
    Unary(UnOp, Box<FeelExpr>),

    // 逻辑短路
    And(Vec<FeelExpr>),
    Or(Vec<FeelExpr>),
    Not(Box<FeelExpr>),

    // 区间 (a..b)
    Interval {
        start: Box<FeelExpr>,
        end: Box<FeelExpr>,
        start_open: bool,
        end_open: bool,
    },

    // 列表
    List(Vec<FeelExpr>),

    // 列表推导 [for x in list: expr]
    ForExpr {
        var: String,
        iter: Box<FeelExpr>,
        body: Box<FeelExpr>,
    },

    // 量化表达式 some x in list satisfies expr / every x in list satisfies expr
    Quantified {
        kind: QuantKind,
        var: String,
        iter: Box<FeelExpr>,
        satisfies: Box<FeelExpr>,
    },

    // if(expr, then, else)
    IfExpr {
        cond: Box<FeelExpr>,
        then_: Box<FeelExpr>,
        else_: Box<FeelExpr>,
    },

    // 函数调用 name(args...)
    Call(String, Vec<FeelExpr>),

    // 上下文 { k: v, ... }
    Context(Vec<(String, FeelExpr)>),

    // 过滤 list[expr]
    Filter(Box<FeelExpr>, Box<FeelExpr>),

    // 路径访问 a.b
    Path(Box<FeelExpr>, String),

    // 索引访问 a[0]：当前 parser 统一产出 Filter，保留节点供解释/格式化匹配
    #[allow(dead_code)]
    Index(Box<FeelExpr>, Box<FeelExpr>),

    // 函数定义 (x, y) -> expr
    FunctionDef(Vec<String>, Box<FeelExpr>),

    // DMN 1.5 新增
    // between expression: x between a and b
    Between {
        value: Box<FeelExpr>,
        low: Box<FeelExpr>,
        high: Box<FeelExpr>,
    },

    // instance of: x instance of Type
    InstanceOf {
        value: Box<FeelExpr>,
        type_name: String,
    },

    // if-then-else keyword form: if cond then a else b
    IfThenElse {
        cond: Box<FeelExpr>,
        then_: Box<FeelExpr>,
        else_: Box<FeelExpr>,
    },

    // @ date/time literal
    AtLiteral(String),

    // is / is not identity comparison
    Is(Box<FeelExpr>, Box<FeelExpr>),
    IsNot(Box<FeelExpr>, Box<FeelExpr>),

    // in with positive unary tests: x in (< 10, >= 5)
    InUnaryTests {
        value: Box<FeelExpr>,
        tests: Vec<UnaryTest>,
    },

    // named parameter function call
    CallNamed {
        name: String,
        args: Vec<(String, FeelExpr)>,
    },
}

// 一元测试 (for "in" with comparison operators)
#[derive(Debug, Clone)]
pub enum UnaryTest {
    LessThan(Box<FeelExpr>),
    LessOrEqual(Box<FeelExpr>),
    GreaterThan(Box<FeelExpr>),
    GreaterOrEqual(Box<FeelExpr>),
    Range(Box<FeelExpr>),
    Value(Box<FeelExpr>),
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
}

#[derive(Debug, Clone, Copy)]
pub enum UnOp {
    #[allow(dead_code)] // Neg（一元 +）：当前 parser 不产出，解释/格式化分支已支持
    Neg,
    Minus,
}

#[derive(Debug, Clone, Copy)]
pub enum QuantKind {
    Some,
    Every,
}
