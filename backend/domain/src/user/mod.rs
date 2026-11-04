// User domain module

pub mod admin_dtos;
pub mod admin_entities;
pub mod admin_errors;
pub mod admin_ports;
pub mod entities;
pub mod phone;
pub mod policies;
pub mod ports;
pub mod services;
pub mod value_objects;

// Basic user exports
pub use entities::{CreateUserConfig, Profile, User, UserStatus};
pub use policies::UserPolicy;
pub use ports::{UserRepository, UserService, UserVerificationRepository};
pub use services::UserServiceImpl;
pub use value_objects::*;

// Admin user exports
pub use admin_dtos::*;
pub use admin_entities::*;
pub use admin_errors::*;
pub use admin_ports::*;
pub use services::admin::*;
