-- Rule-disable deactivation: upsert_rules_in_tx flips every active schedule
-- bound to a disabled distribution rule (WHERE realm_id = $1 AND
-- distribution_rule_id = $2 AND active = TRUE). distribution_rule_id had no
-- index, so the planner could only ride the realm prefix of
-- uq_points_grant_schedules_user_rule and filter the realm's whole schedule
-- population inside the upsert transaction. Partial index in the shape of
-- idx_points_grant_schedules_next_grant_time (0002_billing.sql): only the
-- rule's active rows are touched.
CREATE INDEX idx_points_grant_schedules_realm_rule_active
    ON points_grant_schedules(realm_id, distribution_rule_id)
    WHERE active = TRUE;
