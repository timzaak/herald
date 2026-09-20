pub mod app_errors;
pub mod provider_url;

pub use app_errors::CoreError;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Base entity trait
pub trait Entity {
    fn id(&self) -> Uuid;
    fn created_at(&self) -> DateTime<Utc>;
    fn updated_at(&self) -> DateTime<Utc>;
}

/// Generate UUID v7 (time-ordered, better for database indexing)
/// UUID v7 结合了时间戳和随机数，天生按时间排序，对数据库索引更友好
pub fn generate_uuid_v7() -> Uuid {
    // 使用 now_v7() 自动生成当前时间戳的 UUID v7
    Uuid::now_v7()
}

/// Get current UTC timestamp
pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}
