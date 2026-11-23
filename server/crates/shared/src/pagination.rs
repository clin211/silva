use crate::error::{AppError, FieldError, FieldErrorCode, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PageRequest {
    pub page: u32,
    pub per_page: u32,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 20,
        }
    }
}

impl PageRequest {
    pub fn validate(&self) -> Result<()> {
        let mut errors = Vec::new();
        if self.page == 0 {
            errors.push(FieldError::new(
                "page",
                FieldErrorCode::OutOfRange,
                "page must be at least 1",
            ));
        }
        if !(1..=100).contains(&self.per_page) {
            errors.push(FieldError::new(
                "per_page",
                FieldErrorCode::OutOfRange,
                "per_page must be between 1 and 100",
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation(errors))
        }
    }

    pub fn offset(&self) -> i64 {
        (i64::from(self.page) - 1) * i64::from(self.per_page)
    }

    pub fn metadata(&self, total: i64) -> Pagination {
        Pagination {
            page: self.page,
            per_page: self.per_page,
            total,
            total_pages: (total + i64::from(self.per_page) - 1) / i64::from(self.per_page),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Pagination {
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
    pub total_pages: i64,
}

/// 一页查询结果；API 层将其序列化为 `{"data": items, "meta": {"pagination": ...}}`。
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub pagination: Pagination,
}
