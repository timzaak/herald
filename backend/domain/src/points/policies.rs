// Points Policy - Permission control for points operations
// PermissionBasedPointsPolicy moved to infrastructure/authorization/policies.rs

use crate::authentication::Identity;
use uuid::Uuid;

/// Points Policy - 积分管理权限
#[allow(clippy::manual_async_fn)]
pub trait PointsPolicy: Send + Sync {
    /// 检查用户是否可以查看积分
    fn can_view_points(
        &self,
        identity: Identity,
        target_user_id: Option<Uuid>,
    ) -> impl Future<Output = bool> + Send;

    /// 检查用户是否可以管理积分
    fn can_manage_points(&self, identity: Identity) -> impl Future<Output = bool> + Send;

    /// 检查用户是否可以消耗积分（SDK API）
    fn can_consume_points(&self, identity: Identity) -> impl Future<Output = bool> + Send;

    /// 检查用户是否可以查看积分配置
    fn can_view_points_configs(&self, identity: Identity) -> impl Future<Output = bool> + Send;

    /// 检查用户是否可以管理积分配置
    fn can_manage_points_configs(&self, identity: Identity) -> impl Future<Output = bool> + Send;
}

/// 允许所有策略（开发/测试用）
#[derive(Debug, Clone)]
pub struct AllowAllPointsPolicy;

#[allow(clippy::manual_async_fn)]
impl PointsPolicy for AllowAllPointsPolicy {
    fn can_view_points(
        &self,
        _identity: Identity,
        _target_user_id: Option<Uuid>,
    ) -> impl Future<Output = bool> + Send {
        async move { true }
    }

    fn can_manage_points(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }

    fn can_consume_points(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }

    fn can_view_points_configs(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }

    fn can_manage_points_configs(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
}
