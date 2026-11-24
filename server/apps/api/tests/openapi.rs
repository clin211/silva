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
        ("/api/v1/admin/auth/refresh", "post"),
        ("/api/v1/admin/auth/logout", "post"),
        ("/api/v1/admin/auth/me", "get"),
        ("/api/v1/admin/auth/password", "post"),
        ("/api/v1/admin/users", "get"),
        ("/api/v1/admin/users", "post"),
        ("/api/v1/admin/users/{id}", "get"),
        ("/api/v1/admin/users/{id}", "patch"),
        ("/api/v1/admin/roles", "get"),
        ("/api/v1/admin/audit-logs", "get"),
        ("/api/v1/admin/audit-logs/{id}", "get"),
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
    for schema in schemas.values() {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (field, value) in properties {
                if field == "id" || field.ends_with("_id") {
                    assert_uuid_v7(&spec, value);
                }
            }
        }
    }
    for parameter in [
        "UserId",
        "AuditLogId",
        "AuditActorUserId",
        "AuditResourceId",
        "AuditRequestId",
    ] {
        assert_uuid_v7(
            &spec,
            &spec["components"]["parameters"][parameter]["schema"],
        );
    }
    assert_uuid_v7(&spec, &spec["components"]["headers"]["RequestId"]["schema"]);
}

#[test]
fn bearer_jwt_and_rbac_requirements_are_explicit() {
    let spec = contract();
    assert_eq!(spec["security"], serde_json::json!([{"BearerAuth": []}]));
    let schemes = spec["components"]["securitySchemes"].as_object().unwrap();
    assert_eq!(schemes.len(), 1, "只允许 Bearer 认证");
    let bearer = &schemes["BearerAuth"];
    assert_eq!(bearer["type"], "http");
    assert_eq!(bearer["scheme"], "bearer");
    assert_eq!(bearer["bearerFormat"], "JWT");

    let public = ["/api/v1/admin/auth/login", "/api/v1/admin/auth/refresh"];
    for (path, methods) in spec["paths"].as_object().unwrap() {
        for (method, operation) in methods.as_object().unwrap() {
            if !path.starts_with("/api/") || public.contains(&path.as_str()) {
                assert_eq!(
                    operation["security"],
                    serde_json::json!([]),
                    "{method} {path}"
                );
                continue;
            }
            // 受保护接口沿用全局 Bearer 认证，并声明含 token_expired 的 401。
            assert!(operation.get("security").is_none(), "{method} {path}");
            let unauthorized = resolve(&spec, &operation["responses"]["401"]);
            assert_eq!(
                unauthorized["x-error-codes"],
                serde_json::json!(["unauthorized", "token_expired"]),
                "{method} {path}"
            );
            assert!(unauthorized["headers"].get("WWW-Authenticate").is_some());
        }
    }
    for (path, method, permission) in [
        ("/api/v1/admin/users", "get", "users.manage"),
        ("/api/v1/admin/users", "post", "users.manage"),
        ("/api/v1/admin/users/{id}", "get", "users.manage"),
        ("/api/v1/admin/users/{id}", "patch", "users.manage"),
        ("/api/v1/admin/roles", "get", "roles.read"),
        ("/api/v1/admin/audit-logs", "get", "audit.read"),
        ("/api/v1/admin/audit-logs/{id}", "get", "audit.read"),
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
    for name in [
        "LoginInput",
        "CreateUserInput",
        "UpdateUserInput",
        "ChangePasswordInput",
    ] {
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
    assert_eq!(
        schemas["ChangePasswordInput"]["properties"]["new_password"]["x-min-bytes"],
        12
    );
    assert_eq!(
        spec["paths"]["/api/v1/admin/auth/password"]["post"]["responses"]["204"].get("content"),
        None
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
    // 账户列表筛选参数与运行时校验一致。
    let list_parameters: Vec<_> = spec["paths"]["/api/v1/admin/users"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parameter| resolve(&spec, parameter))
        .collect();
    let parameter = |name: &str| {
        *list_parameters
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap_or_else(|| panic!("账户列表缺少查询参数 {name}"))
    };
    for name in ["page", "per_page", "q", "role", "status"] {
        assert_eq!(parameter(name)["in"], "query");
        assert_eq!(parameter(name)["required"], false);
    }
    assert_eq!(
        parameter("q")["schema"]["x-max-bytes"],
        silva_application::identity::USER_QUERY_MAX_BYTES
    );
    assert_eq!(
        resolve(&spec, &parameter("role")["schema"])["enum"],
        serde_json::json!(silva_domain::identity::ROLE_KEYS)
    );
    assert_eq!(
        resolve(&spec, &parameter("status")["schema"])["enum"],
        serde_json::json!(silva_domain::identity::USER_STATUSES)
    );
    // 资源字段总是存在：可空字段也必须列入 required。
    for (schema, field) in [("User", "last_login_at"), ("Role", "user_count")] {
        assert!(
            schemas[schema]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(field)),
            "{schema}.{field} 必须为必填字段"
        );
    }
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
fn error_code_registries_match_runtime() {
    let spec = contract();
    let schemas = &spec["components"]["schemas"];
    assert_eq!(
        schemas["ErrorCode"]["enum"],
        serde_json::to_value(silva_api::ErrorCode::ALL).unwrap(),
        "OpenAPI ErrorCode 必须与运行时错误码登记表一致"
    );
    assert_eq!(
        schemas["FieldErrorCode"]["enum"],
        serde_json::to_value(silva_api::FieldErrorCode::ALL).unwrap(),
        "OpenAPI FieldErrorCode 必须与运行时字段错误码一致"
    );
    for code in silva_api::FieldErrorCode::ALL {
        assert_eq!(serde_json::to_value(code).unwrap(), code.as_str());
    }
}

/// 响应规范：业务接口成功正文为 `{"data"}` 或 `{"data", "meta"}`；所有错误正文为 ErrorResponse，
/// 且声明的错误码与其 HTTP 状态一致。
#[test]
fn responses_follow_envelope_conventions() {
    let spec = contract();
    let status_of = |code: &Value| {
        silva_api::ErrorCode::ALL
            .into_iter()
            .find(|candidate| serde_json::to_value(candidate).unwrap() == *code)
            .unwrap_or_else(|| panic!("未登记的错误码 {code}"))
            .status()
            .as_u16()
            .to_string()
    };
    for (path, methods) in spec["paths"].as_object().unwrap() {
        for (method, operation) in methods.as_object().unwrap() {
            for (status, response) in operation["responses"].as_object().unwrap() {
                let response = resolve(&spec, response);
                let Some(media) = response["content"].get("application/json") else {
                    continue;
                };
                let schema = resolve(&spec, &media["schema"]);
                if status.starts_with('2') {
                    if path.starts_with("/api/") {
                        let required = schema["required"].as_array().unwrap();
                        assert!(
                            required.contains(&serde_json::json!("data")),
                            "{method} {path} {status}: 成功响应必须以 data 包裹"
                        );
                        let keys: Vec<_> = schema["properties"]
                            .as_object()
                            .unwrap()
                            .keys()
                            .map(String::as_str)
                            .collect();
                        assert!(
                            keys.iter().all(|key| ["data", "meta"].contains(key)),
                            "{method} {path} {status}: 顶层只允许 data 与 meta"
                        );
                        assert_eq!(schema["additionalProperties"], false);
                    }
                } else {
                    assert_eq!(
                        media["schema"]["$ref"], "#/components/schemas/ErrorResponse",
                        "{method} {path} {status}: 错误响应必须使用 ErrorResponse"
                    );
                    let codes = response["x-error-codes"].as_array();
                    if status != "default" {
                        for code in codes.unwrap_or_else(|| {
                            panic!("{method} {path} {status}: 缺少 x-error-codes")
                        }) {
                            assert_eq!(
                                &status_of(code),
                                status,
                                "{method} {path}: 错误码 {code} 的状态不符"
                            );
                        }
                    }
                }
            }
            if operation.get("requestBody").is_some() {
                assert_eq!(
                    operation["responses"]["400"]["$ref"], "#/components/responses/MalformedJson",
                    "{method} {path}: JSON 请求体接口必须声明 400 malformed_json"
                );
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
        production: false,
        jwt_secret: None,
        db_max_connections: 1,
        access_token_ttl: Duration::from_secs(900),
        refresh_token_ttl: Duration::from_secs(3600),
        worker_interval: Duration::from_secs(60),
        api_rate_per_minute: 1000,
        login_rate_per_minute: 100,
        login_lockout_threshold: 5,
        login_lockout_duration: Duration::from_secs(900),
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

    // 请求体与查询参数在访问数据库之前校验，可在无数据库时验证错误结构。
    /// 一条无数据库校验用例。
    struct Case {
        method: &'static str,
        path: &'static str,
        content_type: Option<&'static str>,
        body: &'static str,
        status: StatusCode,
        code: &'static str,
        /// 期望出现的（字段, 字段错误码）；None 表示 details 为空。
        field: Option<(&'static str, &'static str)>,
    }
    const LOGIN: &str = "/api/v1/admin/auth/login";
    let post = |content_type, body, status, code, field| Case {
        method: "POST",
        path: LOGIN,
        content_type,
        body,
        status,
        code,
        field,
    };
    let invalid = StatusCode::UNPROCESSABLE_ENTITY;
    // 审计查询参数在鉴权前校验，非法值不依赖会话即可返回 422。
    let audit = |path, field| Case {
        method: "GET",
        path,
        content_type: None,
        body: "",
        status: invalid,
        code: "validation_error",
        field: Some(field),
    };
    let cases = [
        post(
            Some("application/json"),
            "{",
            StatusCode::BAD_REQUEST,
            "malformed_json",
            None,
        ),
        post(
            Some("application/json"),
            "",
            StatusCode::BAD_REQUEST,
            "malformed_json",
            None,
        ),
        post(
            Some("application/json"),
            r#"{"email":"a@example.test","password":"p"} x"#,
            StatusCode::BAD_REQUEST,
            "malformed_json",
            None,
        ),
        post(
            Some("text/plain"),
            "{}",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            None,
        ),
        post(
            Some("application/json; charset=utf-8"),
            r#"{"email":"a@example.test"}"#,
            invalid,
            "validation_error",
            Some(("password", "required")),
        ),
        post(
            Some("application/json"),
            r#"{"email":"a@example.test","password":"p","otp":1}"#,
            invalid,
            "validation_error",
            Some(("otp", "unknown_field")),
        ),
        post(
            Some("application/json"),
            r#"{"email":1,"password":"p"}"#,
            invalid,
            "validation_error",
            Some(("email", "invalid_type")),
        ),
        post(
            Some("application/json"),
            "[]",
            invalid,
            "validation_error",
            Some(("", "invalid_type")),
        ),
        Case {
            method: "GET",
            path: "/api/v1/admin/users?page=0&per_page=500",
            content_type: None,
            body: "",
            status: invalid,
            code: "validation_error",
            field: Some(("per_page", "out_of_range")),
        },
        audit(
            "/api/v1/admin/audit-logs?from=yesterday",
            ("from", "invalid_format"),
        ),
        audit(
            "/api/v1/admin/audit-logs?from=2026-11-22T00:00:00Z&to=2026-11-21T00:00:00Z",
            ("to", "out_of_range"),
        ),
        audit(
            "/api/v1/admin/audit-logs?from=2026-11-21T00:00:00Z&to=2026-11-21T00:00:00Z",
            ("to", "out_of_range"),
        ),
        audit(
            "/api/v1/admin/audit-logs?actor_user_id=67e55044-10b1-426f-9247-bb680e5fe0c8",
            ("actor_user_id", "invalid_format"),
        ),
        audit(
            "/api/v1/admin/audit-logs?resource_id=x",
            ("resource_id", "invalid_format"),
        ),
        audit(
            "/api/v1/admin/audit-logs?request_id=019535d93df779fbb466fa907fa17f9e",
            ("request_id", "invalid_format"),
        ),
        audit(
            "/api/v1/admin/audit-logs?action=auth*",
            ("action", "invalid_value"),
        ),
        audit(
            "/api/v1/admin/audit-logs?action=Auth.login",
            ("action", "invalid_value"),
        ),
        audit(
            "/api/v1/admin/audit-logs?action=",
            ("action", "invalid_value"),
        ),
        audit(
            "/api/v1/admin/audit-logs?resource_type=user.x",
            ("resource_type", "invalid_value"),
        ),
        audit(
            "/api/v1/admin/audit-logs?action=auth.login&action=auth.logout",
            ("action", "duplicate"),
        ),
        audit(
            "/api/v1/admin/audit-logs/not-a-uuid",
            ("id", "invalid_format"),
        ),
        Case {
            method: "GET",
            path: "/api/v1/admin/users?role=owner&status=locked",
            content_type: None,
            body: "",
            status: invalid,
            code: "validation_error",
            field: Some(("role", "invalid_value")),
        },
        Case {
            method: "GET",
            path: "/api/v1/admin/users?status=locked",
            content_type: None,
            body: "",
            status: invalid,
            code: "validation_error",
            field: Some(("status", "invalid_value")),
        },
        Case {
            method: "GET",
            path: "/api/v1/admin/users/not-an-id",
            content_type: None,
            body: "",
            status: invalid,
            code: "validation_error",
            field: Some(("id", "invalid_format")),
        },
    ];
    for Case {
        method,
        path,
        content_type,
        body,
        status: expected_status,
        code,
        field,
    } in cases
    {
        let method: Method = method.parse().unwrap();
        let mut builder = Request::builder().method(method.clone()).uri(path);
        if let Some(content_type) = content_type {
            builder = builder.header("content-type", content_type);
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        support::assert_response(&method, path, status, &headers, &bytes);
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status, expected_status, "{path} {body}: {json}");
        assert_eq!(json["error"]["code"], code, "{path} {body}: {json}");
        let details = json["error"]["details"].as_array().unwrap();
        match field {
            Some((field, field_code)) => assert!(
                details
                    .iter()
                    .any(|detail| detail["field"] == field && detail["code"] == field_code),
                "{path} {body}: {json}"
            ),
            None => assert!(details.is_empty(), "{path} {body}: {json}"),
        }
        // 字段错误不得回显输入值。
        assert!(!json.to_string().contains("a@example.test"), "{json}");
    }
    telemetry.shutdown().await;
}

/// 请求 ID 只通过响应头返回；认证只使用 Bearer 令牌，契约中不再出现 Cookie、CSRF 或 Origin 校验。
#[test]
fn request_id_is_header_only_and_session_cookie_csrf_origin_are_gone() {
    let spec = contract();
    let schemas = &spec["components"]["schemas"];
    let error = &schemas["ErrorResponse"]["properties"]["error"];
    assert!(error["properties"].get("request_id").is_none());
    assert!(
        !error["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "request_id")
    );
    let text = spec.to_string();
    for removed in [
        "csrf",
        "CSRF",
        "Cookie",
        "origin_rejected",
        "SILVA_ADMIN_ORIGIN",
    ] {
        assert!(!text.contains(removed), "OpenAPI 中不应再出现 {removed}");
    }
}

/// 登录与刷新返回的令牌字段与运行时一致。
#[test]
fn token_responses_declare_every_token_field() {
    let spec = contract();
    let schemas = &spec["components"]["schemas"];
    let fields = serde_json::json!([
        "access_token",
        "refresh_token",
        "token_type",
        "expires_in",
        "refresh_expires_in"
    ]);
    assert_eq!(schemas["TokenPair"]["required"], fields);
    assert_eq!(
        schemas["TokenPair"]["properties"]["token_type"]["const"],
        "Bearer"
    );
    assert_eq!(
        schemas["RefreshInput"]["required"],
        serde_json::json!(["refresh_token"])
    );
    assert_eq!(schemas["RefreshInput"]["additionalProperties"], false);
    let login = support::validator(&schemas["LoginSession"]);
    let mut example = schemas["LoginSession"]["examples"][0].clone();
    assert!(login.is_valid(&example));
    example["unexpected"] = serde_json::json!(true);
    assert!(!login.is_valid(&example), "登录结果不允许额外字段");
    for (path, schema) in [
        ("/api/v1/admin/auth/login", "LoginResponse"),
        ("/api/v1/admin/auth/refresh", "TokenResponse"),
    ] {
        assert_eq!(
            spec["paths"][path]["post"]["responses"]["200"]["content"]["application/json"]["schema"]
                ["$ref"],
            format!("#/components/schemas/{schema}")
        );
    }
}
