use super::*;

#[test]
fn test_literals() {
    assert_eq!(eval_feel("42"), Ok(FeelValue::Number(42.0)));
    assert_eq!(eval_feel("3.14"), Ok(FeelValue::Number(3.14)));
    assert_eq!(eval_feel("true"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("false"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("null"), Ok(FeelValue::Null));
    assert_eq!(
        eval_feel("\"hello\""),
        Ok(FeelValue::String("hello".into()))
    );
    assert_eq!(eval_feel("'world'"), Ok(FeelValue::String("world".into())));
}

#[test]
fn test_arithmetic() {
    assert_eq!(eval_feel("2 + 3"), Ok(FeelValue::Number(5.0)));
    assert_eq!(eval_feel("10 - 4"), Ok(FeelValue::Number(6.0)));
    assert_eq!(eval_feel("3 * 7"), Ok(FeelValue::Number(21.0)));
    assert_eq!(eval_feel("15 / 4"), Ok(FeelValue::Number(3.75)));
    assert_eq!(eval_feel("2 + 3 * 4"), Ok(FeelValue::Number(14.0)));
    assert_eq!(eval_feel("(2 + 3) * 4"), Ok(FeelValue::Number(20.0)));
    assert_eq!(eval_feel("-5"), Ok(FeelValue::Number(-5.0)));
    assert_eq!(eval_feel("-(3 + 2)"), Ok(FeelValue::Number(-5.0)));
}

#[test]
fn test_comparison() {
    assert_eq!(eval_feel("3 > 2"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("3 < 2"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("3 >= 3"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("3 <= 2"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("3 == 3"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("3 != 3"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("\"abc\" == \"abc\""), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("\"abc\" != \"def\""), Ok(FeelValue::Bool(true)));
}

#[test]
fn test_logic() {
    assert_eq!(eval_feel("true and false"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("true or false"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("not true"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("not false"), Ok(FeelValue::Bool(true)));
    // 短路
    assert_eq!(eval_feel("false and true"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("true or false"), Ok(FeelValue::Bool(true)));
}

#[test]
fn test_variables() {
    let mut ctx = HashMap::new();
    ctx.insert("x".into(), FeelValue::Number(10.0));
    ctx.insert("name".into(), FeelValue::String("FEEL".into()));
    assert_eq!(
        eval_feel_with("x + 5", ctx.clone()),
        Ok(FeelValue::Number(15.0))
    );
    assert_eq!(
        eval_feel_with("name", ctx),
        Ok(FeelValue::String("FEEL".into()))
    );
}

#[test]
fn test_if_expr() {
    assert_eq!(
        eval_feel("if(5 > 3, \"yes\", \"no\")"),
        Ok(FeelValue::String("yes".into()))
    );
    assert_eq!(
        eval_feel("if(5 < 3, \"yes\", \"no\")"),
        Ok(FeelValue::String("no".into()))
    );
    assert_eq!(eval_feel("if(null, 1, 2)"), Ok(FeelValue::Number(2.0)));
}

#[test]
fn test_lists() {
    assert_eq!(
        eval_feel("[1, 2, 3]"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::Number(3.0)
        ]))
    );
    // DMN 1.5: 1-based indexing
    assert_eq!(eval_feel("[1, 2, 3][1]"), Ok(FeelValue::Number(1.0)));
    assert_eq!(eval_feel("[1, 2, 3][3]"), Ok(FeelValue::Number(3.0)));
    assert_eq!(eval_feel("[1, 2, 3][0]"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("[1, 2, 3].length"), Ok(FeelValue::Number(3.0)));
}

#[test]
fn test_list_comprehension() {
    let result = eval_feel("[for x in [1, 2, 3]: x * 2]");
    assert_eq!(
        result,
        Ok(FeelValue::List(vec![
            FeelValue::Number(2.0),
            FeelValue::Number(4.0),
            FeelValue::Number(6.0)
        ]))
    );
}

#[test]
fn test_string_functions() {
    assert_eq!(
        eval_feel("string length(\"hello\")"),
        Ok(FeelValue::Number(5.0))
    );
    assert_eq!(
        eval_feel("upper case(\"hello\")"),
        Ok(FeelValue::String("HELLO".into()))
    );
    assert_eq!(
        eval_feel("lower case(\"HELLO\")"),
        Ok(FeelValue::String("hello".into()))
    );
    assert_eq!(
        eval_feel("contains(\"hello\", \"ell\")"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("starts with(\"hello\", \"hel\")"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("ends with(\"hello\", \"llo\")"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("substring(\"hello\", 1, 3)"),
        Ok(FeelValue::String("hel".into()))
    );
    assert_eq!(
        eval_feel("replace(\"hello\", \"l\", \"r\")"),
        Ok(FeelValue::String("herro".into()))
    );
    assert_eq!(
        eval_feel("split(\"a,b,c\", \",\")"),
        Ok(FeelValue::List(vec![
            FeelValue::String("a".into()),
            FeelValue::String("b".into()),
            FeelValue::String("c".into())
        ]))
    );
    assert_eq!(
        eval_feel("string join([\"a\", \"b\", \"c\"], \"-\")"),
        Ok(FeelValue::String("a-b-c".into()))
    );
    assert_eq!(
        eval_feel("\"hello\" + \" \" + \"world\""),
        Ok(FeelValue::String("hello world".into()))
    );
}

#[test]
fn test_list_functions() {
    assert_eq!(eval_feel("count([1, 2, 3, 4])"), Ok(FeelValue::Number(4.0)));
    assert_eq!(eval_feel("sum([1, 2, 3, 4])"), Ok(FeelValue::Number(10.0)));
    assert_eq!(eval_feel("mean([2, 4, 6])"), Ok(FeelValue::Number(4.0)));
    assert_eq!(
        eval_feel("min([3, 1, 4, 1, 5])"),
        Ok(FeelValue::Number(1.0))
    );
    assert_eq!(
        eval_feel("max([3, 1, 4, 1, 5])"),
        Ok(FeelValue::Number(5.0))
    );
    assert_eq!(
        eval_feel("append([1, 2], [3, 4])"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::List(vec![FeelValue::Number(3.0), FeelValue::Number(4.0)])
        ]))
    );
    assert_eq!(
        eval_feel("reverse([1, 2, 3])"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(3.0),
            FeelValue::Number(2.0),
            FeelValue::Number(1.0)
        ]))
    );
    assert_eq!(
        eval_feel("distinct values([1, 2, 2, 3, 3, 3])"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::Number(3.0)
        ]))
    );
    assert_eq!(
        eval_feel("index of([1, 2, 3, 2], 2)"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(2.0),
            FeelValue::Number(4.0)
        ]))
    );
    assert_eq!(
        eval_feel("list contains([1, 2, 3], 2)"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("flatten([[1, 2], [3, [4, 5]]])"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::Number(3.0),
            FeelValue::Number(4.0),
            FeelValue::Number(5.0)
        ]))
    );
}

#[test]
fn test_numeric_functions() {
    assert_eq!(eval_feel("abs(-5)"), Ok(FeelValue::Number(5.0)));
    assert_eq!(eval_feel("floor(3.7)"), Ok(FeelValue::Number(3.0)));
    assert_eq!(eval_feel("ceiling(3.2)"), Ok(FeelValue::Number(4.0)));
    assert_eq!(eval_feel("round(3.5)"), Ok(FeelValue::Number(4.0)));
    assert_eq!(eval_feel("round(3.14159, 2)"), Ok(FeelValue::Number(3.14)));
    assert_eq!(eval_feel("modulo(10, 3)"), Ok(FeelValue::Number(1.0)));
    assert_eq!(eval_feel("sqrt(16)"), Ok(FeelValue::Number(4.0)));
}

#[test]
fn test_in_operator() {
    assert_eq!(eval_feel("3 in [1, 2, 3, 4]"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("5 in [1, 2, 3, 4]"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("3 not in [1, 2, 4]"), Ok(FeelValue::Bool(true)));
    // 区间测试 - 注意: 我们没有直接解析区间语法 (1..10)
    // 但可以通过值来测试
}

#[test]
fn test_quantified() {
    assert_eq!(
        eval_feel("some x in [1, 2, 3] satisfies x > 2"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("some x in [1, 2, 3] satisfies x > 5"),
        Ok(FeelValue::Bool(false))
    );
    assert_eq!(
        eval_feel("every x in [1, 2, 3] satisfies x > 0"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("every x in [1, 2, 3] satisfies x > 2"),
        Ok(FeelValue::Bool(false))
    );
}

#[test]
fn test_context() {
    let result = eval_feel("{name: \"Alice\", age: 30}");
    let expected = {
        let mut m = HashMap::new();
        m.insert("name".into(), FeelValue::String("Alice".into()));
        m.insert("age".into(), FeelValue::Number(30.0));
        FeelValue::Context(m)
    };
    assert_eq!(result, Ok(expected));
}

#[test]
fn test_context_access() {
    let mut ctx = HashMap::new();
    let mut person = HashMap::new();
    person.insert("name".into(), FeelValue::String("Bob".into()));
    person.insert("age".into(), FeelValue::Number(25.0));
    ctx.insert("person".into(), FeelValue::Context(person));

    assert_eq!(
        eval_feel_with("person.name", ctx.clone()),
        Ok(FeelValue::String("Bob".into()))
    );
    assert_eq!(
        eval_feel_with("person.age", ctx),
        Ok(FeelValue::Number(25.0))
    );
}

#[test]
fn test_for_expression() {
    let result = eval_feel("for x in [1, 2, 3] return x * x");
    assert_eq!(
        result,
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(4.0),
            FeelValue::Number(9.0)
        ]))
    );
}

#[test]
fn test_nested_expressions() {
    // 嵌套 if
    assert_eq!(
        eval_feel("if(1 > 2, \"a\", if(3 > 2, \"b\", \"c\"))"),
        Ok(FeelValue::String("b".into()))
    );
    // 列表推导中的条件
    let result = eval_feel("[for x in [1, 2, 3, 4, 5]: if(x > 2, x, null)]");
    assert_eq!(
        result,
        Ok(FeelValue::List(vec![
            FeelValue::Null,
            FeelValue::Null,
            FeelValue::Number(3.0),
            FeelValue::Number(4.0),
            FeelValue::Number(5.0)
        ]))
    );
}

#[test]
fn test_function_def_and_call() {
    let mut ctx = HashMap::new();
    // 在上下文中定义函数
    let double_fn = FeelValue::Function(
        vec!["x".into()],
        FeelExpr::Binary(
            Box::new(FeelExpr::Name("x".into())),
            BinOp::Mul,
            Box::new(FeelExpr::Number(2.0)),
        ),
    );
    ctx.insert("double".into(), double_fn);

    assert_eq!(
        eval_feel_with("double(5)", ctx),
        Ok(FeelValue::Number(10.0))
    );
}

#[test]
fn test_higher_order_functions() {
    // 使用内联函数定义进行 map
    let mut ctx = HashMap::new();
    let square_fn = FeelValue::Function(
        vec!["x".into()],
        FeelExpr::Binary(
            Box::new(FeelExpr::Name("x".into())),
            BinOp::Mul,
            Box::new(FeelExpr::Name("x".into())),
        ),
    );
    ctx.insert("square".into(), square_fn);

    let result = eval_feel_with("map([1, 2, 3, 4], square)", ctx);
    assert_eq!(
        result,
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(4.0),
            FeelValue::Number(9.0),
            FeelValue::Number(16.0)
        ]))
    );
}

#[test]
fn test_type_of() {
    assert_eq!(
        eval_feel("type of(42)"),
        Ok(FeelValue::String("Number".into()))
    );
    assert_eq!(
        eval_feel("type of(\"hi\")"),
        Ok(FeelValue::String("String".into()))
    );
    assert_eq!(
        eval_feel("type of(true)"),
        Ok(FeelValue::String("Boolean".into()))
    );
    assert_eq!(
        eval_feel("type of(null)"),
        Ok(FeelValue::String("Null".into()))
    );
    assert_eq!(
        eval_feel("type of([1,2])"),
        Ok(FeelValue::String("List".into()))
    );
}

#[test]
fn test_conversion() {
    assert_eq!(eval_feel("number(\"42\")"), Ok(FeelValue::Number(42.0)));
    assert_eq!(eval_feel("boolean(\"true\")"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("string(42)"), Ok(FeelValue::String("42".into())));
}

#[test]
fn test_comments() {
    assert_eq!(
        eval_feel("42 // this is a comment"),
        Ok(FeelValue::Number(42.0))
    );
    assert_eq!(eval_feel("// comment\n3 + 4"), Ok(FeelValue::Number(7.0)));
}

#[test]
fn test_complex_business_rule() {
    // 模拟一个贷款审批决策
    let mut ctx = HashMap::new();
    ctx.insert(
        "applicant".into(),
        FeelValue::Context({
            let mut m = HashMap::new();
            m.insert("age".into(), FeelValue::Number(35.0));
            m.insert("creditScore".into(), FeelValue::Number(720.0));
            m.insert("annualIncome".into(), FeelValue::Number(85000.0));
            m.insert("existingDebt".into(), FeelValue::Number(15000.0));
            m
        }),
    );
    ctx.insert("loanAmount".into(), FeelValue::Number(200000.0));

    let rule = r#"
            if(applicant.creditScore >= 750,
                "approved",
                if(applicant.creditScore >= 700 and applicant.annualIncome > 60000,
                    if(loanAmount / applicant.annualIncome <= 3,
                        "approved",
                        "review required"
                    ),
                    if(applicant.creditScore >= 650 and applicant.age >= 25,
                        "review required",
                        "declined"
                    )
                )
            )
        "#;
    let result = eval_feel_with(rule, ctx);
    assert_eq!(result, Ok(FeelValue::String("approved".into())));
}

#[test]
fn test_dmn_style_decision_table_simulation() {
    // 模拟决策表逻辑
    let mut ctx = HashMap::new();
    ctx.insert("riskLevel".into(), FeelValue::String("high".into()));
    ctx.insert("amount".into(), FeelValue::Number(50000.0));

    let rule = r#"
            if(riskLevel == "low" and amount <= 100000, "auto approve",
            if(riskLevel == "medium" and amount <= 50000, "auto approve",
            if(riskLevel == "high" or amount > 100000, "manual review",
                "auto approve"
            )))
        "#;
    let result = eval_feel_with(rule, ctx);
    assert_eq!(result, Ok(FeelValue::String("manual review".into())));
}

#[test]
fn test_string_escape() {
    assert_eq!(
        eval_feel("\"hello\\nworld\""),
        Ok(FeelValue::String("hello\nworld".into()))
    );
    assert_eq!(
        eval_feel("\"tab\\there\""),
        Ok(FeelValue::String("tab\there".into()))
    );
    assert_eq!(
        eval_feel("\"quote\\\"here\""),
        Ok(FeelValue::String("quote\"here".into()))
    );
}

#[test]
fn test_complex_list_operations() {
    // 筛选 + 映射 + 聚合
    let expr = r#"
            sum([for x in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]:
                if(x > 5, x * 2, 0)
            ])
        "#;
    assert_eq!(eval_feel(expr), Ok(FeelValue::Number(80.0))); // 6*2 + 7*2 + 8*2 + 9*2 + 10*2 = 12+14+16+18+20 = 80
}

#[test]
fn test_precedence() {
    // and 优先于 or
    assert_eq!(
        eval_feel("true or false and false"),
        Ok(FeelValue::Bool(true))
    );
    // 比较优先于 and
    assert_eq!(eval_feel("1 > 0 and 2 > 1"), Ok(FeelValue::Bool(true)));
    // 算术优先于比较
    assert_eq!(eval_feel("2 + 3 > 4"), Ok(FeelValue::Bool(true)));
}

#[test]
fn test_sort_function() {
    assert_eq!(
        eval_feel("sort([3, 1, 4, 1, 5])"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(1.0),
            FeelValue::Number(3.0),
            FeelValue::Number(4.0),
            FeelValue::Number(5.0)
        ]))
    );
    assert_eq!(
        eval_feel("sort([3, 1, 4, 1, 5], false)"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(5.0),
            FeelValue::Number(4.0),
            FeelValue::Number(3.0),
            FeelValue::Number(1.0),
            FeelValue::Number(1.0)
        ]))
    );
}

#[test]
fn test_reduce() {
    let mut ctx = HashMap::new();
    let add_fn = FeelValue::Function(
        vec!["acc".into(), "x".into()],
        FeelExpr::Binary(
            Box::new(FeelExpr::Name("acc".into())),
            BinOp::Add,
            Box::new(FeelExpr::Name("x".into())),
        ),
    );
    ctx.insert("add".into(), add_fn);
    assert_eq!(
        eval_feel_with("reduce([1, 2, 3, 4, 5], 0, add)", ctx),
        Ok(FeelValue::Number(15.0))
    );
}

// ========== DMN 1.5 新增测试 ==========

#[test]
fn test_single_equals() {
    // DMN 1.5: 单个 = 也是相等运算符
    assert_eq!(eval_feel("3 = 3"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("3 == 3"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("3 = 4"), Ok(FeelValue::Bool(false)));
}

#[test]
fn test_exponentiation() {
    assert_eq!(eval_feel("2 ** 10"), Ok(FeelValue::Number(1024.0)));
    assert_eq!(eval_feel("2 ** 3 ** 2"), Ok(FeelValue::Number(512.0))); // right-assoc: 2^(3^2)
    assert_eq!(eval_feel("10 ** 0"), Ok(FeelValue::Number(1.0)));
}

#[test]
fn test_between_expression() {
    assert_eq!(eval_feel("5 between 1 and 10"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("15 between 1 and 10"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("1 between 1 and 10"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("10 between 1 and 10"), Ok(FeelValue::Bool(true)));
}

#[test]
fn test_instance_of() {
    assert_eq!(
        eval_feel("42 instance of Number"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("\"hi\" instance of String"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("true instance of Boolean"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("[1,2] instance of List"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("{a:1} instance of Context"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("42 instance of String"),
        Ok(FeelValue::Bool(false))
    );
}

#[test]
fn test_if_then_else() {
    assert_eq!(
        eval_feel("if 5 > 3 then \"yes\" else \"no\""),
        Ok(FeelValue::String("yes".into()))
    );
    assert_eq!(
        eval_feel("if 5 < 3 then \"yes\" else \"no\""),
        Ok(FeelValue::String("no".into()))
    );
}

#[test]
fn test_range_literal() {
    // [1..10] is a range
    let r = eval_feel("[1..10]");
    assert!(matches!(r, Ok(FeelValue::Range { .. })));
}

#[test]
fn test_at_literal() {
    let r = eval_feel("@\"2024-01-01\"");
    assert!(matches!(r, Ok(FeelValue::Date(_))));
}

#[test]
fn test_is_is_not() {
    assert_eq!(eval_feel("5 is 5"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("5 is not 5"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("5 is 6"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("5 is not 6"), Ok(FeelValue::Bool(true)));
}

#[test]
fn test_ternary_logic() {
    // DMN 1.5 ternary logic
    assert_eq!(eval_feel("null and true"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("null and false"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("null or true"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("null or false"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("true and null"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("false or null"), Ok(FeelValue::Null));
}

#[test]
fn test_in_unary_tests() {
    // 5 in (< 10, >= 5) — < 10 is true
    assert_eq!(eval_feel("5 in (< 10)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("15 in (< 10)"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("5 in (>= 5)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("4 in (>= 5)"), Ok(FeelValue::Bool(false)));
}

#[test]
fn test_new_string_functions() {
    assert_eq!(
        eval_feel("substring before(\"foobar\", \"bar\")"),
        Ok(FeelValue::String("foo".into()))
    );
    assert_eq!(
        eval_feel("substring after(\"foobar\", \"foo\")"),
        Ok(FeelValue::String("bar".into()))
    );
    assert_eq!(
        eval_feel("string join([\"a\", \"b\", \"c\"], \"-\")"),
        Ok(FeelValue::String("a-b-c".into()))
    );
    assert_eq!(
        eval_feel("string join([\"a\", null, \"c\"])"),
        Ok(FeelValue::String("ac".into()))
    );
}

#[test]
fn test_new_list_functions() {
    assert_eq!(
        eval_feel("sublist([1, 2, 3, 4, 5], 2, 2)"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(2.0),
            FeelValue::Number(3.0)
        ]))
    );
    assert_eq!(
        eval_feel("insert before([1, 3], 1, 2)"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(2.0),
            FeelValue::Number(1.0),
            FeelValue::Number(3.0)
        ]))
    );
    assert_eq!(
        eval_feel("remove([1, 2, 3], 2)"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(3.0)
        ]))
    );
    assert_eq!(
        eval_feel("list replace([1, 2, 3], 2, 99)"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(99.0),
            FeelValue::Number(3.0)
        ]))
    );
    assert_eq!(
        eval_feel("union([1, 2], [2, 3])"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::Number(3.0)
        ]))
    );
    assert_eq!(eval_feel("product([2, 3, 4])"), Ok(FeelValue::Number(24.0)));
    assert_eq!(
        eval_feel("median([8, 2, 5, 3, 4])"),
        Ok(FeelValue::Number(4.0))
    );
    assert_eq!(
        eval_feel("median([6, 1, 2, 3])"),
        Ok(FeelValue::Number(2.5))
    );
    assert_eq!(eval_feel("all([true, true])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("all([true, false])"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("any([false, true])"), Ok(FeelValue::Bool(true)));
}

#[test]
fn test_new_numeric_functions() {
    assert_eq!(eval_feel("odd(5)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("even(4)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("odd(2)"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("decimal(0.333, 2)"), Ok(FeelValue::Number(0.33)));
    assert_eq!(eval_feel("round up(1.121, 2)"), Ok(FeelValue::Number(1.13)));
    assert_eq!(
        eval_feel("round down(1.129, 2)"),
        Ok(FeelValue::Number(1.12))
    );
}

#[test]
fn test_not_function() {
    // not() function with ternary logic
    assert_eq!(eval_feel("not(true)"), Ok(FeelValue::Bool(false)));
    assert_eq!(eval_feel("not(false)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("not(null)"), Ok(FeelValue::Null));
}

#[test]
fn test_range_iteration() {
    // for x in [1..5] return x*2
    let r = eval_feel("for x in [1..5] return x * 2");
    assert_eq!(
        r,
        Ok(FeelValue::List(vec![
            FeelValue::Number(2.0),
            FeelValue::Number(4.0),
            FeelValue::Number(6.0),
            FeelValue::Number(8.0),
            FeelValue::Number(10.0)
        ]))
    );
}

#[test]
fn test_named_parameters() {
    let r = eval_feel("sort(list: [3, 1, 2], precedes: function(x, y) -> x < y)");
    assert_eq!(
        r,
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::Number(3.0)
        ]))
    );

    let reordered = eval_feel("sort(precedes: function(x, y) -> x < y, list: [3, 1, 2])");
    assert_eq!(reordered, r);
    assert!(eval_feel("sort(items: [1], precedes: function(x, y) -> x < y)").is_err());
}

#[test]
fn test_range_functions() {
    // before, after, coincides
    let r1 = eval_feel("before([1..5], [10..20])");
    assert_eq!(r1, Ok(FeelValue::Bool(true)));
    let r2 = eval_feel("after([11..20], [1..10])");
    assert_eq!(r2, Ok(FeelValue::Bool(true)));
    let r3 = eval_feel("coincides([1..5], [1..5])");
    assert_eq!(r3, Ok(FeelValue::Bool(true)));
    let r4 = eval_feel("includes([1..10], [3..7])");
    assert_eq!(r4, Ok(FeelValue::Bool(true)));
    let r5 = eval_feel("during([3..7], [1..10])");
    assert_eq!(r5, Ok(FeelValue::Bool(true)));
}

#[test]
fn test_temporal_functions() {
    assert_eq!(
        eval_feel("day of year(@\"2019-09-17\")"),
        Ok(FeelValue::Number(260.0))
    );
    assert_eq!(
        eval_feel("month of year(@\"2019-09-17\")"),
        Ok(FeelValue::String("September".into()))
    );
    assert_eq!(
        eval_feel("week of year(@\"2019-09-17\")"),
        Ok(FeelValue::Number(38.0))
    );
}

#[test]
fn test_conversion_functions() {
    // date(y, m, d)
    let d = eval_feel("date(2024, 12, 25)");
    assert!(matches!(d, Ok(FeelValue::Date(_))));
    // time(h, m, s)
    let t = eval_feel("time(23, 59, 0)");
    assert!(matches!(t, Ok(FeelValue::Time(_))));

    let du = eval_feel("duration(\"PT1H\")");

    assert!(matches!(du, Ok(FeelValue::Duration(_))));

    let du = eval_feel("date(\"2017-05-12\") - date(\"2017-04-25\")");
    assert_eq!(du.unwrap(), FeelValue::Duration("P17D".to_string()));
}

#[test]
fn test_context_functions() {
    let r = eval_feel("context([{key: \"a\", value: 1}, {key: \"b\", value: 2}])");
    assert!(matches!(r, Ok(FeelValue::Context(_))));
    let r2 = eval_feel("context put({x: 1}, \"y\", 2)");
    assert!(matches!(r2, Ok(FeelValue::Context(_))));
}

#[test]
fn test_1_based_indexing() {
    // DMN 1.5: 1-based indexing
    assert_eq!(eval_feel("[10, 20, 30][1]"), Ok(FeelValue::Number(10.0)));
    assert_eq!(eval_feel("[10, 20, 30][2]"), Ok(FeelValue::Number(20.0)));
    assert_eq!(eval_feel("[10, 20, 30][3]"), Ok(FeelValue::Number(30.0)));
    assert_eq!(eval_feel("[10, 20, 30][4]"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("[10, 20, 30][-1]"), Ok(FeelValue::Number(30.0)));
}

#[test]
fn dmn15_conversion_functions_preserve_exact_temporal_types() {
    assert_eq!(
        eval_feel("date(2024, 12, 25)"),
        Ok(FeelValue::Date("2024-12-25".into()))
    );
    assert_eq!(
        eval_feel("time(23, 59, 0)"),
        Ok(FeelValue::Time("23:59:00".into()))
    );
    assert_eq!(
        eval_feel("date and time(\"2024-12-25T23:59:00\")"),
        Ok(FeelValue::DateTime("2024-12-25T23:59:00".into()))
    );
}

#[test]
fn dmn15_range_relations_honor_open_endpoints() {
    assert_eq!(eval_feel("before(1, 2)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("before(1, (1..5])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("after(6, [1..5])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("includes([1..5], 3)"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("during(3, [1..5])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("starts(1, [1..5])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("finishes(5, [1..5])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("finishes(5, [1..5))"), Ok(FeelValue::Bool(false)));
    assert_eq!(
        eval_feel("finished by([1..5], 5)"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("finished by([1..5), 5)"),
        Ok(FeelValue::Bool(false))
    );
    assert_eq!(
        eval_feel("meets([1..5], [5..10])"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("meets([1..5), [5..10])"),
        Ok(FeelValue::Bool(false))
    );
    assert_eq!(
        eval_feel("before([1..5), [5..10])"),
        Ok(FeelValue::Bool(true))
    );
    assert!(eval_feel("before(1)").is_err());
}

#[test]
fn dmn15_invalid_expressions_return_errors() {
    for expression in [
        "1 +",
        "unknown variable",
        "unknown function()",
        "number(\"not a number\")",
        "date(2024, 1)",
        "substring(42, 1)",
    ] {
        assert!(eval_feel(expression).is_err(), "{expression}");
    }
}

#[test]
fn renders_and_precompiles_feel_expressions() {
    let compiled = compile_feel("a - (b - c)").unwrap();
    assert_eq!(compiled.expression().to_string(), "a - (b - c)");
    assert_eq!(
        compiled
            .eval_with(&HashMap::from([
                ("a".into(), FeelValue::Number(10.0)),
                ("b".into(), FeelValue::Number(3.0)),
                ("c".into(), FeelValue::Number(1.0)),
            ]))
            .unwrap(),
        FeelValue::Number(8.0)
    );
    assert!(matches!(
        parse_feel(&compiled.expression().to_string()),
        Ok(FeelExpr::Binary(_, BinOp::Sub, _))
    ));
}

#[test]
fn serializes_feel_values_deterministically() {
    let value = FeelValue::Context(HashMap::from([
        ("b".into(), FeelValue::String("quote: \\\"".into())),
        (
            "a".into(),
            FeelValue::DateTime("2024-01-01T12:00:00".into()),
        ),
    ]));
    assert_eq!(
        value.to_feel_string().unwrap(),
        "{a: date and time(\"2024-01-01T12:00:00\"), b: \"quote: \\\\\\\"\"}"
    );
    assert!(FeelValue::Number(f64::NAN).to_feel_string().is_err());
}

#[test]
fn datetime_literals_preserve_their_type() {
    assert_eq!(
        eval_feel("date and time(\"2024-01-01T12:00:00\")").unwrap(),
        FeelValue::DateTime("2024-01-01T12:00:00".into())
    );
}

#[test]
fn dmn15_out_of_range_string_and_list_operations_do_not_panic() {
    for expression in [
        "substring(\"abc\", 4)",
        "substring(\"abc\", -4)",
        "sublist([1, 2, 3], 4)",
        "sublist([1, 2, 3], -4)",
    ] {
        assert!(eval_feel(expression).is_err(), "{expression}");
    }
}

#[test]
fn dmn15_filters_and_list_projection_bind_each_item() {
    assert_eq!(
        eval_feel("[1, 2, 3, 4][item > 2]"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(3.0),
            FeelValue::Number(4.0),
        ]))
    );
    assert_eq!(
        eval_feel("[{x: 1}, {x: 2}, {y: 3}].x"),
        Ok(FeelValue::List(vec![
            FeelValue::Number(1.0),
            FeelValue::Number(2.0),
            FeelValue::Null,
        ]))
    );
    assert_eq!(
        eval_feel("[{x: 1}, {x: 3}][x > 1].x"),
        Ok(FeelValue::List(vec![FeelValue::Number(3.0)]))
    );
}

#[test]
fn dmn15_equality_conditions_and_quantifiers_use_ternary_logic() {
    assert_eq!(eval_feel("\"1\" = 1"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("null = null"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("null = 1"), Ok(FeelValue::Bool(false)));
    assert_eq!(
        eval_feel("if 1 then \"then\" else \"else\""),
        Ok(FeelValue::String("else".into()))
    );
    assert_eq!(
        eval_feel("some x in [1, \"a\"] satisfies x > 2"),
        Ok(FeelValue::Null)
    );
    assert_eq!(
        eval_feel("every x in [1, \"a\"] satisfies x > 0"),
        Ok(FeelValue::Null)
    );
}

#[test]
fn dmn15_string_functions_are_unicode_and_regex_aware() {
    assert_eq!(
        eval_feel("string length(\"你好a\")"),
        Ok(FeelValue::Number(3.0))
    );
    assert_eq!(
        eval_feel("substring(\"你好a\", 2, 2)"),
        Ok(FeelValue::String("好a".into()))
    );
    assert_eq!(
        eval_feel("matches(\"abc123xyz\", \"[0-9]+\")"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("matches(\"AbC\", \"abc\", \"i\")"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("replace(\"abc123\", \"[0-9]+\", \"#\")"),
        Ok(FeelValue::String("abc#".into()))
    );
    assert_eq!(
        eval_feel("split(\"a,  b,c\", \", *\")"),
        Ok(FeelValue::List(vec![
            FeelValue::String("a".into()),
            FeelValue::String("b".into()),
            FeelValue::String("c".into()),
        ]))
    );
}

#[test]
fn dmn15_aggregates_and_rounding_cover_edge_cases() {
    assert_eq!(eval_feel("sum([])"), Ok(FeelValue::Null));
    assert_eq!(eval_feel("sum(1, 2, 3)"), Ok(FeelValue::Number(6.0)));
    assert_eq!(eval_feel("mean(2, 4, 6)"), Ok(FeelValue::Number(4.0)));
    assert_eq!(eval_feel("all([])"), Ok(FeelValue::Bool(true)));
    assert_eq!(eval_feel("any([])"), Ok(FeelValue::Bool(false)));
    assert_eq!(
        eval_feel("mode([6, 3, 9, 6, 6, 9])"),
        Ok(FeelValue::List(vec![FeelValue::Number(6.0)]))
    );
    assert_eq!(eval_feel("decimal(2.5, 0)"), Ok(FeelValue::Number(2.0)));
    assert_eq!(eval_feel("decimal(3.5, 0)"), Ok(FeelValue::Number(4.0)));
    assert_eq!(eval_feel("round up(-1.21, 1)"), Ok(FeelValue::Number(-1.3)));
    assert_eq!(
        eval_feel("round down(-1.29, 1)"),
        Ok(FeelValue::Number(-1.2))
    );
    assert_eq!(eval_feel("sqrt(-1)"), Ok(FeelValue::Null));
}

#[test]
fn dmn15_temporal_and_nested_context_functions_return_spec_types() {
    assert!(matches!(eval_feel("now()"), Ok(FeelValue::DateTime(_))));
    assert!(matches!(eval_feel("today()"), Ok(FeelValue::Date(_))));
    assert_eq!(
        eval_feel("day of week(date(2024, 1, 1))"),
        Ok(FeelValue::String("Monday".into()))
    );
    assert_eq!(
        eval_feel("context put({a: {b: 1}}, [\"a\", \"b\"], 2).a.b"),
        Ok(FeelValue::Number(2.0))
    );
}

#[test]
fn dmn15_temporal_constructors_validate_calendar_and_clock_values() {
    assert_eq!(
        eval_feel("date(2024, 2, 29)"),
        Ok(FeelValue::Date("2024-02-29".into()))
    );
    assert!(eval_feel("date(2023, 2, 29)").is_err());
    assert!(eval_feel("date(2024, 13, 1)").is_err());
    assert!(eval_feel("date(2024.5, 1, 1)").is_err());

    assert_eq!(
        eval_feel("time(23, 59, 59.5)"),
        Ok(FeelValue::Time("23:59:59.5".into()))
    );
    assert!(eval_feel("time(24, 0, 0)").is_err());
    assert!(eval_feel("time(12, 60, 0)").is_err());
    assert!(eval_feel("time(12, 0, 60)").is_err());

    assert_eq!(
        eval_feel("date and time(date(2024, 2, 29), time(12, 30, 15.25))"),
        Ok(FeelValue::DateTime("2024-02-29T12:30:15.25".into()))
    );
    assert_eq!(
        eval_feel("date(date and time(\"2024-02-29T12:30:15+08:00\"))"),
        Ok(FeelValue::Date("2024-02-29".into()))
    );
    assert_eq!(
        eval_feel("time(date and time(\"2024-02-29T12:30:15+08:00\"))"),
        Ok(FeelValue::Time("12:30:15+08:00".into()))
    );
    assert!(eval_feel("date and time(\"2024-02-30T12:00:00\")").is_err());
}

#[test]
fn dmn15_temporal_properties_preserve_fractional_seconds_and_timezones() {
    assert_eq!(
        eval_feel("date(2024, 2, 29).year"),
        Ok(FeelValue::Number(2024.0))
    );
    assert_eq!(
        eval_feel("date(2024, 2, 29).month"),
        Ok(FeelValue::Number(2.0))
    );
    assert_eq!(
        eval_feel("date(2024, 2, 29).day"),
        Ok(FeelValue::Number(29.0))
    );
    assert_eq!(
        eval_feel("time(12, 30, 15.25).hour"),
        Ok(FeelValue::Number(12.0))
    );
    assert_eq!(
        eval_feel("time(12, 30, 15.25).second"),
        Ok(FeelValue::Number(15.25))
    );
    assert_eq!(
        eval_feel("date and time(\"2024-02-29T12:30:15.25+08:00\").minute"),
        Ok(FeelValue::Number(30.0))
    );
    assert_eq!(
        eval_feel("@\"12:30:00-05:00\".hour"),
        Ok(FeelValue::Number(12.0))
    );
}

#[test]
fn dmn15_temporal_comparison_normalizes_timezone_offsets() {
    assert_eq!(
        eval_feel(
            "date and time(\"2024-01-01T08:00:00+08:00\") = date and time(\"2024-01-01T00:00:00Z\")"
        ),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("time(\"08:00:00+08:00\") = time(\"00:00:00Z\")"),
        Ok(FeelValue::Bool(true))
    );
    assert_eq!(
        eval_feel("date(2024, 1, 1) < date(2024, 1, 2)"),
        Ok(FeelValue::Bool(true))
    );
}

#[test]
fn dmn15_duration_validation_properties_and_signs() {
    assert_eq!(
        eval_feel("duration(\"P1Y2M\").years"),
        Ok(FeelValue::Number(1.0))
    );
    assert_eq!(
        eval_feel("duration(\"P1Y2M\").months"),
        Ok(FeelValue::Number(2.0))
    );
    assert_eq!(
        eval_feel("duration(\"P14M\").years"),
        Ok(FeelValue::Number(1.0))
    );
    assert_eq!(
        eval_feel("duration(\"P14M\").months"),
        Ok(FeelValue::Number(2.0))
    );
    assert_eq!(
        eval_feel("duration(\"P2DT3H4M5.5S\").seconds"),
        Ok(FeelValue::Number(5.5))
    );
    assert_eq!(
        eval_feel("duration(\"-P2DT3H\").hours"),
        Ok(FeelValue::Number(-3.0))
    );
    assert_eq!(
        eval_feel("duration(\"P1DT25H\").days"),
        Ok(FeelValue::Number(2.0))
    );
    assert_eq!(
        eval_feel("duration(\"P1DT25H\").hours"),
        Ok(FeelValue::Number(1.0))
    );
    assert!(eval_feel("duration(\"P\")").is_err());
    assert!(eval_feel("duration(\"P1Y2D\")").is_err());
    assert_eq!(
        eval_feel("years and months duration(date(2020, 1, 1), date(2021, 3, 1))"),
        Ok(FeelValue::Duration("P1Y2M".into()))
    );
    assert_eq!(
        eval_feel("years and months duration(date(2021, 3, 1), date(2020, 1, 1))"),
        Ok(FeelValue::Duration("-P1Y2M".into()))
    );
    assert_eq!(
        eval_feel("date(2024, 1, 1) - date(2024, 1, 2)"),
        Ok(FeelValue::Duration("-P1D".into()))
    );
}

#[test]
fn dmn15_calendar_functions_reject_invalid_inputs() {
    assert_eq!(
        eval_feel("day of year(date(2024, 12, 31))"),
        Ok(FeelValue::Number(366.0))
    );
    assert_eq!(
        eval_feel("week of year(date(2021, 1, 1))"),
        Ok(FeelValue::Number(53.0))
    );
    assert_eq!(
        eval_feel("month of year(date(2024, 12, 1))"),
        Ok(FeelValue::String("December".into()))
    );
    assert!(eval_feel("day of week(\"invalid\")").is_err());
    assert!(eval_feel("month of year(\"2024-13-01\")").is_err());
}

#[test]
fn test_new_list_functions_0001() {
    let expr = r#"
    if  Pre-Bureau Risk Category = "DECLINE" or
        Installment Affordable = false or
        Age < 18 or
        Monthly Income < 100
    then "INELIGIBLE" else "ELIGIBLE"
    "#;
    let compiled = compile_feel(expr).unwrap();
    println!("expr={}", compiled.expression().to_string());

    let result = compiled
        .eval_with(&HashMap::from([
            ("Age".into(), FeelValue::Number(10.0)),
            (
                "Pre-Bureau Risk Category".into(),
                FeelValue::String("DECLINE".into()),
            ),
        ]))
        .unwrap();
    assert_eq!(result, FeelValue::String("INELIGIBLE".into()));

    let expr = "{Principal: 12, Rate: 1, Fees: 1, Term: -1, R: (Principal*Rate/12)/(1-(1+Rate/12)**-Term)+Fees}.R";
    let result = eval_feel(expr).unwrap();
    println!("expr={:?}", result);

    let expr = r#"
        {
          performanceBaseProduct: performanceScore * baseSalary,
          bonus: if performanceBaseProduct > 10000 then performanceBaseProduct * 0.1 else performanceBaseProduct * 0.05
        }.bonus
    "#;
    let result = eval_feel_with(
        expr,
        HashMap::from([
            ("performanceScore".into(), FeelValue::Number(200.0)),
            ("baseSalary".into(), FeelValue::Number(10.0)),
        ]),
    )
    .unwrap();
    println!("expr={:?}", result);
}
