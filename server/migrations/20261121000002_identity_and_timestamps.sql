-- 补充账户约束，并由数据库统一维护更新时间。
ALTER TABLE users ADD CONSTRAINT users_status_check
  CHECK (status IN ('active', 'disabled'));
CREATE UNIQUE INDEX users_email_lower_unique ON users (lower(email));
COMMENT ON COLUMN users.status IS '账户状态：active 可登录、disabled 已停用；停用用例在同一事务撤销会话。';

CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_expires_at_idx ON sessions(expires_at);
CREATE INDEX audit_logs_created_at_idx ON audit_logs(created_at DESC, id DESC);

CREATE FUNCTION maintain_updated_at() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  NEW.updated_at := now();
  RETURN NEW;
END;
$$;
COMMENT ON FUNCTION maintain_updated_at() IS '在更新已有 updated_at 字段的业务记录时维护本次事务更新时间。';

DO $$
DECLARE
  business_table regclass;
BEGIN
  FOR business_table IN
    SELECT c.oid::regclass
    FROM pg_class AS c
    JOIN pg_namespace AS n ON n.oid = c.relnamespace
    JOIN pg_attribute AS a ON a.attrelid = c.oid
    WHERE n.nspname = 'public' AND c.relkind = 'r'
      AND c.relname <> '_sqlx_migrations'
      AND a.attname = 'updated_at' AND a.attnum > 0 AND NOT a.attisdropped
  LOOP
    EXECUTE format(
      'CREATE TRIGGER maintain_updated_at BEFORE UPDATE ON %s FOR EACH ROW EXECUTE FUNCTION maintain_updated_at()',
      business_table
    );
  END LOOP;
END;
$$;
