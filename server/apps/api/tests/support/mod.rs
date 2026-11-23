//! 按 OpenAPI 3.1 契约校验测试中的真实响应，复用完整 JSON Schema 校验器。

use axum::http::{HeaderMap, Method, StatusCode};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock};

static SPEC: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("../../openapi.json")).unwrap());

static VALIDATORS: LazyLock<BTreeMap<String, jsonschema::Validator>> = LazyLock::new(|| {
    let mut validators = BTreeMap::new();
    for (path, methods) in SPEC["paths"].as_object().unwrap() {
        for (method, operation) in methods.as_object().unwrap() {
            for (status, response) in operation["responses"].as_object().unwrap() {
                let response = resolve(response);
                if let Some(content) = response["content"].as_object() {
                    for (media, content) in content {
                        validators.insert(
                            format!("{method} {path} {status} {media}"),
                            validator(&content["schema"]),
                        );
                    }
                }
            }
        }
    }
    validators
});

fn resolve(value: &Value) -> &Value {
    value
        .get("$ref")
        .and_then(Value::as_str)
        .map_or(value, |reference| {
            SPEC.pointer(reference.strip_prefix('#').unwrap()).unwrap()
        })
}

pub fn validator(schema: &Value) -> jsonschema::Validator {
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "components": SPEC["components"],
            "allOf": [schema],
        }))
        .expect("契约中的 Schema 必须有效，且只引用本地定义")
}

pub fn assert_response(
    method: &Method,
    path: &str,
    status: StatusCode,
    headers: &HeaderMap,
    bytes: &[u8],
) {
    let path = path.split('?').next().unwrap();
    let template = SPEC["paths"].as_object().unwrap().keys().find(|template| {
        let expected: Vec<_> = template.split('/').collect();
        let actual: Vec<_> = path.split('/').collect();
        expected.len() == actual.len()
            && expected
                .iter()
                .zip(actual)
                .all(|(expected, actual)| expected.starts_with('{') || *expected == actual)
    });
    let method = method.as_str().to_ascii_lowercase();
    let operation = template.and_then(|template| SPEC["paths"][template].get(&method));
    let status_key = status.as_u16().to_string();
    let response_key = operation.map(|operation| {
        if operation["responses"].get(&status_key).is_some() {
            status_key.as_str()
        } else {
            assert!(status.is_client_error() || status.is_server_error());
            "default"
        }
    });
    let response = operation
        .zip(response_key)
        .map(|(operation, key)| resolve(&operation["responses"][key]));
    if let Some(response) = response {
        if let Some(expected_headers) = response["headers"].as_object() {
            for (name, expected) in expected_headers {
                let value = headers
                    .get(name)
                    .unwrap_or_else(|| panic!("{method} {path}: 缺少响应头 {name}"))
                    .to_str()
                    .unwrap();
                let schema = &resolve(expected)["schema"];
                let value = if schema["type"] == "integer" {
                    json!(value.parse::<i64>().expect("响应头必须为整数"))
                } else {
                    json!(value)
                };
                assert!(validator(schema).is_valid(&value));
            }
        }
        if response.get("content").is_none() {
            assert!(bytes.is_empty(), "{method} {path}: 响应不应有正文");
            return;
        }
    }
    let media = headers["content-type"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let body: Value = if media == "application/json" {
        serde_json::from_slice(bytes).unwrap()
    } else {
        json!(std::str::from_utf8(bytes).unwrap())
    };
    if let Some(codes) = response.and_then(|response| response["x-error-codes"].as_array()) {
        assert!(
            codes.contains(&body["error"]["code"]),
            "{method} {path}: 错误码未在该响应中声明"
        );
    }
    if let Some((template, response_key)) = template.zip(response_key) {
        let key = format!("{method} {template} {response_key} {media}");
        let validator = VALIDATORS
            .get(&key)
            .unwrap_or_else(|| panic!("{key}: 响应媒体类型未在契约中声明"));
        let errors: Vec<_> = validator
            .iter_errors(&body)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{key}: {errors:?}");
    } else {
        // 未知路径和不支持的方法也必须遵循统一错误结构。
        assert!(status.is_client_error());
        assert!(validator(&SPEC["components"]["schemas"]["ErrorResponse"]).is_valid(&body));
    }
}
