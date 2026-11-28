-- 通过 CLI 在事务中执行，调用方会回滚全部校验操作。
-- 调用方通过 shared::ids 生成两个不同的 UUIDv7，并设置以下事务变量：
-- 首个测试标识：SELECT set_config('silva.test_id', $1, true)
-- 另一个测试标识：SELECT set_config('silva.test_other_id', $2, true)
DO $$
DECLARE
  missing_objects text;
  entity_table record;
  column_names text;
  column_values text;
  probe_sql text;
  test_id uuid := current_setting('silva.test_id')::uuid;
  other_test_id uuid := current_setting('silva.test_other_id')::uuid;
  bad_id uuid;
  violated_constraint text;
  entity_count integer := 0;
BEGIN
  IF NOT is_uuid_v7(test_id) OR NOT is_uuid_v7(other_test_id) OR test_id = other_test_id THEN
    RAISE EXCEPTION 'silva.test_id and silva.test_other_id must be distinct UUIDv7 values from shared::ids';
  END IF;

  SELECT string_agg(expected_name, ', ' ORDER BY expected_name)
  INTO missing_objects
  FROM unnest(ARRAY[
    'users', 'roles', 'permissions', 'user_roles', 'role_permissions', 'sessions',
    'posts', 'post_revisions', 'categories', 'tags', 'post_revision_categories',
    'post_revision_tags', 'published_posts', 'published_post_categories', 'published_post_tags', 'published_post_media', 'media_assets', 'media_blobs', 'media_blob_writes', 'media_object_deletions', 'media_references', 'post_revision_media', 'profiles',
    'experiences', 'skills', 'social_links', 'ai_sources', 'ai_repositories',
    'ai_ingestion_runs', 'ai_repository_snapshots', 'site_state',
    'audit_logs', 'series', 'series_sections', 'series_items'
  ]) AS expected(expected_name)
  WHERE to_regclass(format('public.%I', expected_name)) IS NULL;
  IF missing_objects IS NOT NULL THEN
    RAISE EXCEPTION 'Missing schema tables: %', missing_objects;
  END IF;

  -- 已取消的目录、项目展览与工具导航表必须由清理迁移删除，避免旧库漏执行迁移却通过校验。
  SELECT string_agg(removed_name, ', ' ORDER BY removed_name)
  INTO missing_objects
  FROM unnest(ARRAY[
    'post_reviews', 'mcp_servers', 'mcp_server_versions', 'mcp_categories', 'mcp_server_categories',
    'projects', 'project_technologies', 'project_links', 'tools', 'publication_history_archive', 'outbox_events', 'publish_jobs', 'live_post_media', 'site_generation_posts', 'site_generation_series'
  ]) AS removed(removed_name)
  WHERE to_regclass(format('public.%I', removed_name)) IS NOT NULL;
  IF missing_objects IS NOT NULL THEN
    RAISE EXCEPTION 'Removed schema tables still exist: %', missing_objects;
  END IF;

  SELECT string_agg(object_name, ', ' ORDER BY object_name)
  INTO missing_objects
  FROM (
    SELECT format('%I.%I', n.nspname, c.relname) AS object_name
    FROM pg_class AS c
    JOIN pg_namespace AS n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')
      AND c.relname <> '_sqlx_migrations'
      AND regexp_replace(coalesce(obj_description(c.oid, 'pg_class'), ''), '\s', '', 'g') = ''
    UNION ALL
    SELECT format('%I.%I.%I', n.nspname, c.relname, a.attname)
    FROM pg_class AS c
    JOIN pg_namespace AS n ON n.oid = c.relnamespace
    JOIN pg_attribute AS a ON a.attrelid = c.oid
    WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')
      AND c.relname <> '_sqlx_migrations' AND a.attnum > 0 AND NOT a.attisdropped
      AND regexp_replace(coalesce(col_description(c.oid, a.attnum), ''), '\s', '', 'g') = ''
  ) AS undocumented;
  IF missing_objects IS NOT NULL THEN
    RAISE EXCEPTION 'Missing or blank database comments: %', missing_objects;
  END IF;

  IF is_uuid_v7('019535d9-3df7-49fb-b466-fa907fa17f9e'::uuid)
    OR is_uuid_v7('019535d9-3df7-79fb-7466-fa907fa17f9e'::uuid)
    OR is_uuid_v7('019535d9-3df7-79fb-c466-fa907fa17f9e'::uuid) THEN
    RAISE EXCEPTION 'is_uuid_v7 accepts a v4 or non-RFC variant UUID';
  END IF;

  -- 复制各实体的实际 CHECK 约束和默认值，不复制外键。
  -- 这样无需构造完整的业务关联数据，即可校验每个实体的 CHECK 约束。
  FOR entity_table IN
    SELECT c.oid, c.oid::regclass AS qualified_name, c.relname
    FROM pg_class AS c
    JOIN pg_namespace AS n ON n.oid = c.relnamespace
    JOIN pg_constraint AS pk ON pk.conrelid = c.oid AND pk.contype = 'p'
    JOIN pg_attribute AS a ON a.attrelid = c.oid AND a.attnum = pk.conkey[1]
    WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')
      AND cardinality(pk.conkey) = 1 AND a.atttypid = 'uuid'::regtype
    ORDER BY c.relname
  LOOP
    entity_count := entity_count + 1;
    IF NOT EXISTS (
      SELECT 1 FROM pg_constraint AS con
      WHERE con.conrelid = entity_table.oid AND con.contype = 'c'
        AND pg_get_constraintdef(con.oid) LIKE '%is_uuid_v7(id)%'
    ) THEN
      RAISE EXCEPTION 'UUIDv7 primary-key CHECK missing: %.id', entity_table.qualified_name;
    END IF;

    EXECUTE format(
      'CREATE TEMP TABLE silva_uuid_probe (LIKE %s INCLUDING DEFAULTS INCLUDING CONSTRAINTS) ON COMMIT DROP',
      entity_table.qualified_name
    );
    SELECT string_agg(format('%I', a.attname), ', ' ORDER BY a.attnum),
      string_agg(
        CASE
          WHEN a.attname = 'id' THEN '$1'
          -- 未删除账户需要密码哈希，即使该列允许已删除账户使用 NULL。
          WHEN entity_table.relname = 'users' AND a.attname = 'password_hash' THEN quote_literal('silva-schema-probe')
          WHEN NOT a.attnotnull THEN 'NULL'
          WHEN d.oid IS NOT NULL THEN 'DEFAULT'
          WHEN a.atttypid = 'uuid'::regtype THEN quote_literal(test_id) || '::uuid'
          WHEN a.atttypid = 'text'::regtype THEN
            CASE WHEN a.attname LIKE '%mime_type' THEN quote_literal('image/png')
            WHEN a.attname = 'status' THEN
              quote_literal('queued')
            ELSE quote_literal('silva-schema-probe') END
          WHEN a.atttypid IN ('int2'::regtype, 'int4'::regtype, 'int8'::regtype) THEN '1'
          WHEN a.atttypid = 'timestamptz'::regtype THEN 'now()'
          WHEN a.atttypid = 'date'::regtype THEN 'current_date'
          WHEN a.atttypid = 'bool'::regtype THEN 'true'
          WHEN a.atttypid = 'jsonb'::regtype THEN quote_literal('{}') || '::jsonb'
          ELSE 'NULL'
        END, ', ' ORDER BY a.attnum
      )
    INTO column_names, column_values
    FROM pg_attribute AS a
    LEFT JOIN pg_attrdef AS d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    WHERE a.attrelid = entity_table.oid AND a.attnum > 0 AND NOT a.attisdropped;
    probe_sql := format('INSERT INTO silva_uuid_probe (%s) VALUES (%s)', column_names, column_values);
    EXECUTE probe_sql USING test_id;

    FOREACH bad_id IN ARRAY ARRAY[
      '019535d9-3df7-49fb-b466-fa907fa17f9e'::uuid,
      '019535d9-3df7-79fb-7466-fa907fa17f9e'::uuid,
      '019535d9-3df7-79fb-c466-fa907fa17f9e'::uuid
    ] LOOP
      BEGIN
        EXECUTE probe_sql USING bad_id;
        RAISE EXCEPTION 'Invalid UUID accepted by %.id: %', entity_table.qualified_name, bad_id;
      EXCEPTION WHEN check_violation THEN
        NULL;
      END;
    END LOOP;
    DROP TABLE silva_uuid_probe;
  END LOOP;
  IF entity_count < 23 THEN
    RAISE EXCEPTION 'UUID entity coverage unexpectedly low: %', entity_count;
  END IF;

  -- 逐列核对系列外键的删除策略，篇章的复合外键不能退化为单列引用。
  IF EXISTS (
    SELECT 1 FROM (VALUES
      ('series','cover_media_id','media_assets','id','r'), ('series','category_id','categories','id','r'),
      ('series','created_by','users','id','a'), ('series_sections','series_id','series','id','c'),
      ('series_items','series_id','series','id','c'), ('series_items','post_id','posts','id','c'),
      ('series_items','series_id,section_id','series_sections','series_id,id','a')
    ) AS e(src,cols,dst,refs,policy)
    WHERE NOT EXISTS (
      SELECT 1 FROM pg_constraint c WHERE c.contype='f' AND c.conrelid=e.src::regclass
        AND c.confrelid=e.dst::regclass AND c.confdeltype::text=e.policy
        AND (SELECT string_agg(a.attname,',' ORDER BY k.ord) FROM unnest(c.conkey) WITH ORDINALITY k(n,ord)
             JOIN pg_attribute a ON a.attrelid=c.conrelid AND a.attnum=k.n)=e.cols
        AND (SELECT string_agg(a.attname,',' ORDER BY k.ord) FROM unnest(c.confkey) WITH ORDINALITY k(n,ord)
             JOIN pg_attribute a ON a.attrelid=c.confrelid AND a.attnum=k.n)=e.refs
    )
  ) THEN RAISE EXCEPTION '系列外键或删除策略不匹配'; END IF;
  IF EXISTS (
    SELECT 1 FROM unnest(ARRAY['series','series_sections','series_items']) t(name)
    WHERE NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid=t.name::regclass
      AND tgfoid='maintain_updated_at()'::regprocedure AND tgtype=19 AND tgenabled='O')
      OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid=t.name::regclass
        AND contype='p' AND pg_get_constraintdef(oid)='PRIMARY KEY (id)')
  )
  THEN RAISE EXCEPTION '系列主键或更新时间触发器不匹配'; END IF;

  -- 两个系列中的同位置篇章不能互借；移到根后可删除原篇章而保留文章。
  INSERT INTO series(id,slug,title) VALUES(test_id,test_id::text,'系列甲'),(other_test_id,other_test_id::text,'系列乙');
  INSERT INTO series_sections(id,series_id,title,position) VALUES(test_id,test_id,'第一章',0);
  INSERT INTO posts(id,slug,current_revision_id) VALUES(test_id,test_id::text,test_id);
  INSERT INTO post_revisions(id,post_id,revision_no,title,markdown_body) VALUES(test_id,test_id,1,'成员','');
  BEGIN
    INSERT INTO series_items(id,series_id,section_id,post_id,position) VALUES(test_id,other_test_id,test_id,test_id,0);
    RAISE EXCEPTION '目录接受了其他系列的篇章';
  EXCEPTION WHEN foreign_key_violation THEN NULL;
  END;
  INSERT INTO series_items(id,series_id,section_id,post_id,position) VALUES(test_id,test_id,test_id,test_id,0);
  UPDATE series_items SET section_id=NULL WHERE id=test_id;
  DELETE FROM series_sections WHERE id=test_id;
  IF NOT EXISTS (SELECT 1 FROM series_items WHERE series_id=test_id AND section_id IS NULL AND post_id=test_id)
    THEN RAISE EXCEPTION '删除篇章丢失根条目'; END IF;
  DELETE FROM series WHERE id IN (test_id,other_test_id);
  IF NOT EXISTS (SELECT 1 FROM posts WHERE id=test_id)
    THEN RAISE EXCEPTION '删除系列丢失文章'; END IF;

  -- 在真实业务表上校验无外键的 UUID 引用，不仅依赖通过 LIKE 复制的临时表。
  INSERT INTO users (id, email, password_hash, display_name)
  VALUES (test_id, test_id::text || '@schema.invalid', 'not-a-real-password-hash', 'Schema probe');
  INSERT INTO audit_logs (id, actor_user_id, action, resource_type, resource_id)
  VALUES (test_id, test_id, 'schema_probe', 'schema_probe', test_id);
  FOREACH bad_id IN ARRAY ARRAY[
    '019535d9-3df7-49fb-b466-fa907fa17f9e'::uuid,
    '019535d9-3df7-79fb-7466-fa907fa17f9e'::uuid,
    '019535d9-3df7-79fb-c466-fa907fa17f9e'::uuid
  ] LOOP
    BEGIN
      UPDATE audit_logs SET resource_id = bad_id WHERE id = test_id;
      RAISE EXCEPTION 'Invalid UUID accepted by audit_logs.resource_id: %', bad_id;
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
      UPDATE audit_logs SET request_id = bad_id WHERE id = test_id;
      RAISE EXCEPTION 'Invalid UUID accepted by audit_logs.request_id: %', bad_id;
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
      UPDATE users SET id = bad_id WHERE id = test_id;
      RAISE EXCEPTION 'Invalid UUID accepted by users.id: %', bad_id;
    EXCEPTION WHEN check_violation THEN NULL;
    END;
  END LOOP;

  IF EXISTS (SELECT FROM pg_attribute WHERE attrelid='posts'::regclass
      AND attname IN ('desired_published_revision_id','desired_published_at','published_revision_id','published_at','post_type','slug_locked')
      AND NOT attisdropped) OR EXISTS (SELECT FROM pg_attribute WHERE attrelid='series'::regclass
      AND attname IN ('desired_published_at','slug_locked') AND NOT attisdropped) THEN
    RAISE EXCEPTION '文章或系列仍存有已弃用的发布字段';
  END IF;
  IF (SELECT count(*) FROM pg_constraint
      WHERE conname = 'posts_current_revision_fk'
        AND conrelid = 'posts'::regclass AND contype = 'f'
        AND condeferrable AND condeferred AND confdeltype = 'a') <> 1 THEN
    RAISE EXCEPTION 'posts current revision FK must use deferred NO ACTION';
  END IF;
  IF (SELECT count(*) FROM site_state WHERE singleton) <> 1
    OR EXISTS (SELECT 1 FROM site_state WHERE NOT singleton) THEN
    RAISE EXCEPTION 'site_state must contain exactly the singleton=true row';
  END IF;
  BEGIN
    INSERT INTO site_state(singleton) VALUES (false);
    RAISE EXCEPTION 'site_state accepted singleton=false';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    UPDATE users SET status = 'invalid_status' WHERE id = test_id;
    RAISE EXCEPTION 'users.status accepted an unsupported state';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    INSERT INTO users(id, email, password_hash, display_name)
    VALUES (other_test_id,
      upper(test_id::text || '@schema.invalid'), 'not-a-real-password-hash', 'Duplicate email probe');
    RAISE EXCEPTION 'users.email accepted a case-insensitive duplicate';
  EXCEPTION WHEN unique_violation THEN
    GET STACKED DIAGNOSTICS violated_constraint = CONSTRAINT_NAME;
    IF violated_constraint <> 'users_email_lower_unique' THEN
      RAISE EXCEPTION 'Expected users_email_lower_unique rejection, got %', violated_constraint;
    END IF;
  END;
  UPDATE users SET updated_at = '2000-01-01T00:00:00Z'::timestamptz WHERE id = test_id;
  IF (SELECT updated_at FROM users WHERE id = test_id) <= '2000-01-01T00:00:00Z'::timestamptz THEN
    RAISE EXCEPTION 'users.updated_at trigger did not maintain the update time';
  END IF;

  -- 媒体状态一致性：ready 必须有尺寸，rejected 必须有拒绝原因和拒绝时间，声明类型限定为允许的图片格式。
  INSERT INTO media_assets (id, upload_key, original_filename, declared_mime_type,
    declared_size_bytes, upload_expires_at, uploaded_by)
  VALUES (other_test_id, 'uploads/' || other_test_id, 'probe.png', 'image/png', 1, now(), test_id);
  BEGIN
    UPDATE media_assets SET status = 'ready', storage_key = 'media/' || other_test_id,
      mime_type = 'image/png', size_bytes = 1 WHERE id = other_test_id;
    RAISE EXCEPTION 'media_assets accepted ready without dimensions';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    UPDATE media_assets SET status = 'rejected' WHERE id = other_test_id;
    RAISE EXCEPTION 'media_assets accepted rejected without a reason';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    UPDATE media_assets SET status = 'rejected', rejection_reason = 'too_large' WHERE id = other_test_id;
    RAISE EXCEPTION 'media_assets accepted rejected without rejected_at';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    UPDATE media_assets SET rejection_reason = 'too_large' WHERE id = other_test_id;
    RAISE EXCEPTION 'media_assets accepted a rejection reason on a pending record';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    UPDATE media_assets SET declared_mime_type = 'image/svg+xml' WHERE id = other_test_id;
    RAISE EXCEPTION 'media_assets accepted an unsupported declared MIME type';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  UPDATE media_assets SET updated_at = '2000-01-01T00:00:00Z'::timestamptz WHERE id = other_test_id;
  IF (SELECT updated_at FROM media_assets WHERE id = other_test_id) <= '2000-01-01T00:00:00Z'::timestamptz THEN
    RAISE EXCEPTION 'media_assets.updated_at trigger did not maintain the update time';
  END IF;
END;
$$;
