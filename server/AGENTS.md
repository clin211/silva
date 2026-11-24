# 服务端代码约定

`server/` 中的代码注释必须使用中文，包括 Rust 文档注释、测试说明、SQL 注释和配置说明。技术术语、代码标识符及工具要求的语法指令保留原文。

服务端开发与校验优先采用 Rust 生态工具，不为 API 文档引入 Node.js 工具链。API 文档交付 OpenAPI JSON，在线预览和调试使用 Apifox，服务端不内嵌预览页面。

所有 HTTP API 必须符合 RESTful 风格：路径只用资源名词，动作由 HTTP 方法表达，规则见 [RESTful 接口设计规范](docs/contracts/restful-api.md)。新增或修改接口前先按该规范确定资源与方法。
