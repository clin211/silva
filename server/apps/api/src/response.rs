//! 统一成功响应：`{"data": ...}`，列表附带 `{"meta": {"pagination": ...}}`。
//! 规范见 `docs/contracts/api-response.md`；业务处理器只通过本模块构造 2xx 响应。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use silva_shared::pagination::{Page, Pagination};

#[derive(Serialize)]
struct DataBody<T> {
    data: T,
}

#[derive(Serialize)]
struct ListBody<T> {
    data: Vec<T>,
    meta: ListMeta,
}

#[derive(Serialize)]
struct ListMeta {
    pagination: Pagination,
}

/// 200，返回单个资源、未分页集合或操作结果。
pub(crate) fn ok<T: Serialize>(data: T) -> Response {
    Json(DataBody { data }).into_response()
}

/// 201，返回新建的资源。
pub(crate) fn created<T: Serialize>(data: T) -> Response {
    (StatusCode::CREATED, Json(DataBody { data })).into_response()
}

/// 200，返回一页集合与分页元数据。
pub(crate) fn page<T: Serialize>(page: Page<T>) -> Response {
    Json(ListBody {
        data: page.items,
        meta: ListMeta {
            pagination: page.pagination,
        },
    })
    .into_response()
}

/// 204，操作成功且没有可返回的内容。
pub(crate) fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}
