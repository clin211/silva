use crate::error::{AppError, Result};
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
        if self.page == 0 || !(1..=100).contains(&self.per_page) {
            return Err(AppError::Validation(
                "page must be positive and per_page must be 1..100".into(),
            ));
        }
        Ok(())
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

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}
