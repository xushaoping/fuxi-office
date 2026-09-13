创建一个名为 OrderDiscountCalculation 的订单折扣计算模型。

输入：OrderAmount（number，订单金额元）、CustomerLevel（string，客户等级）、ProductType（string，商品类型）、MaxDiscountLimit（number，业务折扣上限）。

先创建两个 BKM（业务知识模型）：
1. BKM_CalculateAmountDiscount，逻辑为 OrderAmount / 10000，结果为 number。
2. BKM_CalculateLevelDiscount，逻辑为 如果 CustomerLevel 等于 "VIP" 则为 0.15，否则为 0.05，结果为 number。

创建 AmountDiscount 决策，调用 BKM_CalculateAmountDiscount，传入 OrderAmount，结果为 number。
创建 LevelDiscount 决策，调用 BKM_CalculateLevelDiscount，传入 CustomerLevel，结果为 number。

再创建 BaseDiscount 决策，使用决策表，输入 AmountDiscount、LevelDiscount 和 ProductType，
输出为 number。规则按 FIRST 顺序：
1. ProductType 等于 "Clothing"，输出 AmountDiscount + LevelDiscount + 0.05；
2. 其他情况输出 AmountDiscount + LevelDiscount。

创建 FinalDiscount 决策，使用 FIRST 决策表，依赖 BaseDiscount 和 MaxDiscountLimit：
1. BaseDiscount 大于 MaxDiscountLimit，输出 MaxDiscountLimit；
2. 其他情况输出 BaseDiscount。

最后创建一个名为 OrderDiscountService 的 decision service，输出决策为 FinalDiscount，将其依赖的输入、BKM 和中间决策封装在内。