# 百炼视频冻结样例

这些是server a4ea39cb纯函数转录、官方接口文档示例和边界合成，不是线上抓包。
prepare/render对应旧managed视频行为；HappyHorse usage.duration与UNKNOWN过期语义来自
2026-09-20核查的官方文档（链接见设计记录）；冲突/零/非法计量与路径异常为合成负例。
原生与Wasm使用相同pack；不得从被测实现重新生成expected掩盖回归。
