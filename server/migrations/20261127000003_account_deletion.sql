-- 保留原账户 ID，使内容署名和历史审计在删除后仍可追溯。
ALTER TABLE users
  ADD COLUMN deleted_at timestamptz,
  ALTER COLUMN password_hash DROP NOT NULL,
  ADD CONSTRAINT users_deleted_credentials_check CHECK (
    (deleted_at IS NULL AND password_hash IS NOT NULL) OR
    (deleted_at IS NOT NULL AND password_hash IS NULL AND status = 'disabled')
  );
COMMENT ON COLUMN users.email IS '登录邮箱，在未删除账户中忽略大小写唯一。';
COMMENT ON COLUMN users.password_hash IS 'Argon2id 密码哈希，删除账户后清空。';
COMMENT ON COLUMN users.deleted_at IS '账户删除时间；已删除账户仅用于历史归属。';

-- 邮箱可以用于新账户；同邮箱的新旧账户通过不同 ID 区分。
ALTER TABLE users DROP CONSTRAINT users_email_key;
DROP INDEX users_email_lower_unique;
CREATE UNIQUE INDEX users_email_lower_unique ON users (lower(email)) WHERE deleted_at IS NULL;
