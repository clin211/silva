# OpenAPI 校验 Schema

`openapi-3.1-schema.json` 原样保存自 OpenAPI Initiative 官方的固定版本：

https://spec.openapis.org/oas/3.1/schema/2025-09-15

该 Schema 校验 OpenAPI 3.1 文档结构，不校验文档内的 Schema Object；后者由 Rust `jsonschema` 测试单独编译校验，并用于验证示例和真实响应。测试不在线下载 Schema。
