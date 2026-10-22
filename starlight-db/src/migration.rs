//! SQL snippets shared by service migrations.

/// Enables and forces row-level security so even the table owner only sees the current tenant.
pub fn enable_tenant_isolation(table: &str) -> String {
    format!(
        "ALTER TABLE {table} ENABLE ROW LEVEL SECURITY;
         ALTER TABLE {table} FORCE ROW LEVEL SECURITY;
         CREATE POLICY tenant_isolation ON {table}
             USING (tenant_id = NULLIF(current_setting('{setting}', true), '')::uuid)
             WITH CHECK (tenant_id = NULLIF(current_setting('{setting}', true), '')::uuid);",
        setting = crate::TENANT_SETTING,
    )
}

/// Trigger keeping `updated_at` current on every UPDATE.
pub const TOUCH_UPDATED_AT_FUNCTION: &str = "
CREATE OR REPLACE FUNCTION touch_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;";

pub fn touch_updated_at_trigger(table: &str) -> String {
    format!(
        "CREATE TRIGGER trg_{table}__touch_updated_at BEFORE UPDATE ON {table}
         FOR EACH ROW EXECUTE FUNCTION touch_updated_at();"
    )
}
