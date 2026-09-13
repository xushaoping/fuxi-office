创建一个名为 LoanApprovalProcess 的贷款审批模型。

输入：CreditScore（number，信用评分）、MonthlyIncome（number，月收入元）、OverdueCount（number，逾期次数）、LoanAmount（number，贷款金额元）。

先创建两个 BKM（业务知识模型）：
1. BKM_CalculateRiskScore，逻辑为 OverdueCount * 30 + (700 - CreditScore)，结果为 number。
2. BKM_CalculateIncomeRatio，逻辑为 LoanAmount / (MonthlyIncome * 12)，结果为 number。

创建 RiskScore 决策，调用 BKM_CalculateRiskScore，传入 CreditScore 和 OverdueCount，结果为 number。
创建 IncomeRatio 决策，调用 BKM_CalculateIncomeRatio，传入 LoanAmount 和 MonthlyIncome，结果为 number。

再创建 RiskLevel 决策，使用决策表，输入 RiskScore 和 OverdueCount，
输出为 string。规则按 FIRST 顺序：
1. RiskScore 大于 100，或 OverdueCount 大于 3，输出 "High"；
2. RiskScore 大于 50，或 OverdueCount 大于 1，输出 "Medium"；
3. 其他情况输出 "Low"。

创建 ApprovalEligibility 决策，使用 FIRST 决策表，依赖 CreditScore、MonthlyIncome、IncomeRatio：
1. CreditScore 小于 600，输出 "Reject"；
2. MonthlyIncome 小于 5000，输出 "Reject"；
3. IncomeRatio 大于 0.5，输出 "ManualReview"；
4. 其他情况输出 "Eligible"。

创建 RejectReason 决策，使用 FIRST 决策表，依赖 CreditScore、MonthlyIncome、OverdueCount：
1. CreditScore 小于 600，输出 "Credit score too low"；
2. MonthlyIncome 小于 5000，输出 "Income does not meet minimum requirement"；
3. OverdueCount 大于 3，输出 "Too many overdue records"；
4. 其他情况输出 "N/A"。

最后创建 LoanDecision 决策，依赖 RiskLevel、ApprovalEligibility 和 RejectReason，并使用 FIRST 决策表：
1. ApprovalEligibility 为 "Reject"，输出 RejectReason；
2. RiskLevel 为 "High"，输出 "High risk rejected"；
3. ApprovalEligibility 为 "ManualReview"，输出 "ManualReview"；
4. RiskLevel 为 "Medium"，输出 "ManualReview"；
5. 其他情况输出 "Approve"。

最后创建一个名为 LoanApprovalService 的 decision service，输出决策为 LoanDecision，将其依赖的输入、BKM 和中间决策封装在内。