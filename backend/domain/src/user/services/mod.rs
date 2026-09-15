// User services module
//
// This module contains service implementations organized by responsibility:
// - basic: Services for user's own operations (login, register, password change)
// - admin: Services for admin operations (create users, assign roles, manage permissions)

pub mod admin;
pub mod basic;
pub mod self_delete;

// Re-export basic user services for backward compatibility
pub use basic::UserServiceImpl;
pub use basic::ensure_password_byte_length;
pub use self_delete::SelfDeleteService;
