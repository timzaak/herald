pub mod create;
pub mod delete;
pub mod get;
pub mod list;
pub mod permissions;
pub mod phone;
pub mod reset_password;
pub mod roles;
pub mod sessions;
pub mod types;
pub mod update;

use axum::{
    Router,
    routing::{delete, get, post},
};
use herald_api_base::application::http::state::AppState;

// Re-export for utoipa
pub use create::__path_create_user;
pub use delete::__path_delete_user;
pub use get::__path_get_user;
pub use list::__path_list_users;
pub use phone::__path_admin_user_phone_rebind_confirm;
pub use phone::__path_admin_user_phone_rebind_send;
pub use phone::__path_admin_user_phone_unbind;
pub use reset_password::__path_reset_user_password;
pub use sessions::__path_list_user_sessions;
pub use sessions::__path_revoke_all_user_sessions;
pub use sessions::__path_revoke_user_session;
pub use update::__path_update_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list::list_users).post(create::create_user))
        .route(
            "/{userId}",
            get(get::get_user)
                .put(update::update_user)
                .delete(delete::delete_user),
        )
        .route(
            "/{userId}/reset-password",
            post(reset_password::reset_user_password),
        )
        .route(
            "/{userId}/roles",
            get(roles::get_user_roles).put(roles::update_user_roles),
        )
        .route(
            "/{userId}/permissions",
            get(permissions::get_user_permissions)
                .post(permissions::assign_user_permission)
                .delete(permissions::remove_user_permission),
        )
        .route(
            "/{userId}/effective-permissions",
            get(permissions::get_effective_permissions),
        )
        .route(
            "/{userId}/sessions",
            get(sessions::list_user_sessions).delete(sessions::revoke_all_user_sessions),
        )
        .route(
            "/{userId}/sessions/{familyId}",
            delete(sessions::revoke_user_session),
        )
        // Phone disposition (phone login, US-PL-005)
        .route("/{userId}/phone", delete(phone::admin_user_phone_unbind))
        .route(
            "/{userId}/phone/rebind/send",
            post(phone::admin_user_phone_rebind_send),
        )
        .route(
            "/{userId}/phone/rebind/confirm",
            post(phone::admin_user_phone_rebind_confirm),
        )
}
