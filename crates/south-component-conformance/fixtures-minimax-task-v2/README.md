# MiniMax v1 冻结样例

源自 server a8dd0ace 的 Hailuo v1 合法字段/状态转录，
不是在线抓包；期望值独立写定，不从组件输出生成。
覆盖 task-v2 七类方法，费用留宿主、用量缺失保持 null。

## H3 扩展（未发布0.30候选）

H3正反样例来自server现有纯协议函数转录，使用合成数据，不是上游抓包。
覆盖规范分辨率/图片数、别名原model、固定locator、原ID编码、缺失/非法实际秒数、
排队/运行/失败/取消、直链render与无需artifact fetch。Hailuo样例补Task5新字段。
