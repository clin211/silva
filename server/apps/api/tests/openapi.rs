use serde_json::Value;
use std::collections::BTreeSet;

mod support;

const UUID_V7_PATTERN: &str =
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-7[0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$";

fn contract() -> Value {
    serde_json::from_str(include_str!("../openapi.json")).expect("valid OpenAPI JSON")
}

#[test]
fn openapi_document_matches_official_schema() {
    // 固定官方 Schema 快照，校验过程不访问网络；业务 Schema 在后续测试单独校验。
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/openapi-3.1-schema.json")).unwrap();
    let validator = jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap();
    let spec = contract();
    let errors: Vec<_> = validator
        .iter_errors(&spec)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "OpenAPI 文档结构无效：{errors:?}");

    for pointer in [
        "/info/title",
        "/paths/~1api~1v1~1admin~1auth~1login/post/responses",
    ] {
        let mut invalid = spec.clone();
        *invalid.pointer_mut(pointer).unwrap() = serde_json::json!(42);
        assert!(!validator.is_valid(&invalid), "应拒绝无效结构：{pointer}");
    }
}

#[test]
fn every_component_schema_is_valid() {
    for schema in contract()["components"]["schemas"]
        .as_object()
        .unwrap()
        .values()
    {
        support::validator(schema);
    }
}

fn resolve<'a>(root: &'a Value, value: &'a Value) -> &'a Value {
    match value.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let pointer = reference
                .strip_prefix('#')
                .expect("contract references must be local");
            root.pointer(pointer)
                .unwrap_or_else(|| panic!("unresolved reference: {reference}"))
        }
        None => value,
    }
}

fn visit_references(root: &Value, value: &Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("$ref") {
                resolve(root, value);
            }
            for child in object.values() {
                visit_references(root, child);
            }
        }
        Value::Array(array) => {
            for child in array {
                visit_references(root, child);
            }
        }
        _ => {}
    }
}

#[test]
fn contract_has_no_broken_references_and_covers_every_exposed_route() {
    let spec = contract();
    assert_eq!(spec["openapi"], "3.1.0");
    visit_references(&spec, &spec);

    let expected = [
        ("/health/live", "get"),
        ("/health/ready", "get"),
        ("/metrics", "get"),
        ("/openapi.json", "get"),
        ("/api/v1/admin/auth/login", "post"),
        ("/api/v1/admin/auth/logout", "post"),
        ("/api/v1/admin/auth/me", "get"),
        ("/api/v1/admin/users", "get"),
        ("/api/v1/admin/users", "post"),
        ("/api/v1/admin/users/{id}", "patch"),
        ("/api/v1/admin/roles", "get"),
        ("/api/v1/admin/audit-logs", "get"),
        ("/api/v1/admin/system/health", "get"),
    ];
    let paths = spec["paths"].as_object().expect("paths object");
    let actual: BTreeSet<_> = paths
        .iter()
        .flat_map(|(path, operations)| {
            operations
                .as_object()
                .expect("path item")
                .keys()
                .map(move |method| (path.as_str(), method.as_str()))
        })
        .collect();
    assert_eq!(actual, expected.into_iter().collect());

    let mut operation_ids = BTreeSet::new();
    for (path, method) in actual {
        let operation = &spec["paths"][path][method];
        assert!(
            operation_ids.insert(operation["operationId"].as_str().expect("operation ID")),
            "operation IDs must be unique"
        );
        assert!(operation["responses"].get("default").is_some());
    }
    assert!(
        spec["paths"]["/api/v1/admin/auth/logout"]["post"]["responses"]["204"]
            .get("content")
            .is_none()
    );
}

fn assert_uuid_v7(root: &Value, schema: &Value) {
    let schema = resolve(root, schema);
    // 可空引用也必须保留相同的 UUID 版本和变体约束。
    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[1]["type"], "null");
        assert_uuid_v7(root, &variants[0]);
        return;
    }
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["format"], "uuid");
    assert_eq!(schema["pattern"], UUID_V7_PATTERN);
}

#[test]
fn all_entity_and_request_ids_constrain_uuid_version_and_variant() {
    let spec = contract();
    let schemas = spec["components"]["schemas"].as_object().unwrap();
    assert_uuid_v7(&spec, &schemas["UuidV7"]);
    for (name, schema) in schemas {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (field, value) in properties {
                if field == "id" || field.ends_with("_id") {
                    assert_uuid_v7(&spec, value);
                }
            }
        }
        if name == "ErrorResponse" {
            assert_uuid_v7(
                &spec,
                &schema["properties"]["error"]["properties"]["request_id"],
            );
        }
    }
    assert_uuid_v7(&spec, &spec["components"]["parameters"]["UserId"]["schema"]);
    assert_uuid_v7(&spec, &spec["components"]["headers"]["RequestId"]["schema"]);
}

#[test]
fn session_csrf_origin_and_rbac_requirements_are_explicit() {
    let spec = contract();
    assert_eq!(spec["security"][0]["SessionCookie"], serde_json::json!([]));
    let cookie = &spec["components"]["securitySchemes"]["SessionCookie"];
    assert_eq!(cookie["type"], "apiKey");
    assert_eq!(cookie["in"], "cookie");
    assert_eq!(cookie["name"], "__Host-silva_session");

    for (path, methods) in spec["paths"].as_object().unwrap() {
        for (method, operation) in methods.as_object().unwrap() {
            if !path.starts_with("/api/") || path.ends_with("/auth/login") {
                assert_eq!(operation["security"], serde_json::json!([]));
            }
            if method == "post" || method == "patch" {
                let parameters: Vec<_> = operation["parameters"]
                    .as_array()
                    .expect("write headers documented")
                    .iter()
                    .map(|parameter| resolve(&spec, parameter))
                    .collect();
                assert!(parameters.iter().any(|parameter| {
                    parameter["name"] == "Origin" && parameter["required"] == true
                }));
                if !path.ends_with("/auth/login") {
                    assert!(parameters.iter().any(|parameter| {
                        parameter["name"] == "X-CSRF-Token" && parameter["required"] == true
                    }));
                }
            }
        }
    }
    for (path, method, permission) in [
        ("/api/v1/admin/users", "get", "users.manage"),
        ("/api/v1/admin/users", "post", "users.manage"),
        ("/api/v1/admin/users/{id}", "patch", "users.manage"),
        ("/api/v1/admin/roles", "get", "roles.read"),
        ("/api/v1/admin/audit-logs", "get", "audit.read"),
        ("/api/v1/admin/system/health", "get", "system.read"),
    ] {
        assert_eq!(
            spec["paths"][path][method]["x-required-permission"],
            permission
        );
    }
}

#[test]
fn input_shapes_and_pagination_match_runtime_validation() {
    let spec = contract();
    let schemas = &spec["components"]["schemas"];
    for name in ["LoginInput", "CreateUserInput", "UpdateUserInput"] {
        assert_eq!(schemas[name]["additionalProperties"], false);
    }
    assert_eq!(
        schemas["LoginInput"]["required"],
        serde_json::json!(["email", "password"])
    );
    assert_eq!(
        schemas["UpdateUserInput"]["required"],
        serde_json::json!([])
    );
    assert_eq!(
        schemas["CreateUserInput"]["properties"]["password"]["x-min-bytes"],
        12
    );
    assert_eq!(
        schemas["CreateUserInput"]["properties"]["password"]["x-max-bytes"],
        1024
    );
    assert_eq!(schemas["RoleAssignments"]["uniqueItems"], true);
    assert_eq!(schemas["RoleAssignments"]["minItems"], 1);
    assert_eq!(
        spec["components"]["parameters"]["Page"]["schema"]["default"],
        1
    );
    assert_eq!(
        spec["components"]["parameters"]["PerPage"]["schema"]["default"],
        20
    );
    assert_eq!(
        spec["components"]["parameters"]["PerPage"]["schema"]["maximum"],
        100
    );
}

#[test]
fn documented_examples_match_their_schemas() {
    fn visit(value: &Value) {
        if let Some(object) = value.as_object() {
            if let Some(schema) = object.get("schema")
                && let Some(example) = object.get("example")
            {
                let validator = support::validator(schema);
                let errors: Vec<_> = validator
                    .iter_errors(example)
                    .map(|error| error.to_string())
                    .collect();
                assert!(errors.is_empty(), "示例不符合 Schema: {errors:?}");
            }
            if let Some(examples) = object.get("examples").and_then(Value::as_array) {
                let validator = support::validator(value);
                for example in examples {
                    assert!(validator.is_valid(example), "Schema 示例不符合约束");
                }
            }
            for child in object.values() {
                visit(child);
            }
        } else if let Some(array) = value.as_array() {
            for child in array {
                visit(child);
            }
        }
    }
    let spec = contract();
    visit(&spec);
    for (path, methods) in spec["paths"].as_object().unwrap() {
        for operation in methods.as_object().unwrap().values() {
            assert!(operation["summary"].as_str().is_some_and(|s| !s.is_empty()));
            if let Some(body) = operation.get("requestBody") {
                assert!(body["content"]["application/json"].get("example").is_some());
            }
            for response in operation["responses"].as_object().unwrap().values() {
                let response = resolve(&spec, response);
                if let Some(codes) = response["x-error-codes"].as_array() {
                    let code = &response["content"]["application/json"]["example"]["error"]["code"];
                    assert!(codes.contains(code), "{path}: 示例错误码不符合响应定义");
                }
                if path != "/openapi.json"
                    && let Some(content) = response["content"].as_object()
                {
                    for media in content.values() {
                        assert!(media.get("example").is_some(), "{path}: 缺少响应示例");
                    }
                }
            }
        }
    }
}

#[test]
fn response_schema_validation_rejects_contract_drift() {
    let spec = contract();
    let validator = support::validator(&spec["components"]["schemas"]["User"]);
    let mut user = spec["components"]["schemas"]["User"]["examples"][0].clone();
    assert!(validator.is_valid(&user));
    user["id"] = serde_json::json!("550e8400-e29b-41d4-a716-446655440000");
    assert!(!validator.is_valid(&user), "UUIDv4 不符合契约");
    user["id"] = spec["components"]["schemas"]["User"]["examples"][0]["id"].clone();
    user.as_object_mut().unwrap().remove("email");
    assert!(!validator.is_valid(&user), "缺少必填字段必须失败");
}

#[tokio::test]
async fn contract_and_declared_routes_are_served_without_database_access() {
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use silva_shared::{config::Config, telemetry};
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;
    use tower::ServiceExt;

    let telemetry = telemetry::init_local("silva-openapi-test").unwrap();
    let database_url = "postgres://unused:unused@127.0.0.1:1/unused";
    let pool = PgPoolOptions::new().connect_lazy(database_url).unwrap();
    pool.close().await;
    let config = Config {
        database_url: database_url.into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        admin_origin: "https://admin.example.test".into(),
        db_max_connections: 1,
        session_ttl: Duration::from_secs(3600),
        worker_interval: Duration::from_secs(60),
        api_rate_per_minute: 1000,
        login_rate_per_minute: 100,
    };
    let app = silva_api::build_router(config, pool).await.unwrap();
    let spec = contract();
    for (template, methods) in spec["paths"].as_object().unwrap() {
        for method in methods.as_object().unwrap().keys() {
            let method: Method = method.to_ascii_uppercase().parse().unwrap();
            let path = template.replace("{id}", "019535d9-3df7-79fb-b466-fa907fa17f9e");
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(&path)
                        .header("origin", "https://admin.example.test")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            assert_ne!(status, StatusCode::NOT_FOUND, "{method} {path}");
            assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            support::assert_response(&method, &path, status, &headers, &bytes);
            if path == "/openapi.json" {
                assert_eq!(status, StatusCode::OK);
                assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), spec);
            }
        }
    }
    telemetry.shutdown().await;
}
