创建一个名为 FlightDelayResolution 的航班延误处理模型。

输入：DelayHours（number，航班延误时长小时）、CabinClass（string，旅客舱位）、IsConnectingFlight（boolean，是否联程）。

先创建两个 BKM（业务知识模型）：
1. BKM_CalculateBaseCompensation，逻辑为 DelayHours * 100，结果为 number。
2. BKM_CalculateCabinMultiplier，逻辑为 if CabinClass = "First" then 2.0 else if CabinClass = "Business" then 1.5 else 1.0，结果为 number。

创建 BaseCompensation 决策，调用 BKM_CalculateBaseCompensation，传入 DelayHours，结果为 number。
创建 CabinMultiplier 决策，调用 BKM_CalculateCabinMultiplier，传入 CabinClass，结果为 number。

再创建 CompensationAmount 决策，使用决策表，输入 BaseCompensation 和 CabinMultiplier，
输出为 number。规则按 FIRST 顺序：
1. BaseCompensation 大于 0，输出 BaseCompensation * CabinMultiplier；
2. 其他情况输出 0。

创建 FlightAction 决策，使用 FIRST 决策表，依赖 DelayHours 和 IsConnectingFlight：
1. DelayHours 大于 4，输出 "Refund"；
2. IsConnectingFlight 等于 true 且 DelayHours 大于 2，输出 "Rebook"；
3. DelayHours 大于 1，输出 "Compensate"；
4. 其他情况输出 "Wait"。

最后创建 FinalResolution 决策，依赖 FlightAction 和 CompensationAmount，并使用 FIRST 决策表：
1. FlightAction 为 "Refund"，输出 "Full Refund"；
2. FlightAction 为 "Rebook"，输出 "Free Rebook"；
3. FlightAction 为 "Compensate"，输出 "Compensation: " + string(CompensationAmount) + " CNY"；
4. 其他情况输出 "No Action"。

最后创建一个名为 FlightDelayService 的 decision service，输出决策为 FinalResolution，将其依赖的输入、BKM 和中间决策封装在内。