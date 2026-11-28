//! 同步发布升级的三个旧库状态复用外层测试的独立数据库，每个情形结束时回滚。
use super::*;
use silva_infrastructure::persistence::MIGRATOR;

const RECONCILE: i64 = 20261127000000;

/// 用生产迁移入口直接从多修订版本升级；拒绝后旧库还能读取历史并提交审核。
pub(super) async fn check_original_schema(pool: &PgPool, post: Uuid, draft: Uuid) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO publish_jobs(id,generation,status,trigger_type) VALUES($1,2,'queued','manual')")
        .bind(ids::new_id()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE site_state SET desired_generation=2")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let baseline: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap();
    let error = silva_infrastructure::persistence::migrate(pool)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("SSR 切换已中止"));
    let mut tx = pool.begin().await.unwrap();
    let versions: (i64, i64, i64) = sqlx::query_as(
        "SELECT active_generation,desired_generation,
        (SELECT max(version) FROM _sqlx_migrations) FROM site_state",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(versions, (1, 2, baseline));
    let history: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM post_revisions r
        LEFT JOIN post_reviews v ON v.revision_id=r.id WHERE r.post_id=$1",
    )
    .bind(post)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(history, 2);
    sqlx::query(
        "INSERT INTO post_reviews(id,post_id,revision_id,status) VALUES($1,$2,$3,'pending')",
    )
    .bind(ids::new_id())
    .bind(post)
    .bind(draft)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE site_state SET desired_generation=active_generation")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM publish_jobs WHERE generation=2")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn blocked(tx: &mut PgConnection) {
    let error = MIGRATOR.run(&mut *tx).await.unwrap_err();
    assert!(error.to_string().contains("SSR 切换已中止"), "{error}");
    let recorded: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=$1)")
            .bind(RECONCILE)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(!recorded, "处理完待发布事项后必须能重新检查");
    let state: (i64, i64) =
        sqlx::query_as("SELECT active_generation, desired_generation FROM site_state")
            .fetch_one(tx)
            .await
            .unwrap();
    assert_eq!(state, (1, 2));
}

pub(super) async fn check(pool: &PgPool, live_post: Uuid, draft_revision: Uuid) {
    // 类型不明的旧任务保留人工核对入口。
    let mut connection = pool.acquire().await.unwrap();
    let mut tx = connection.begin().await.unwrap();
    sqlx::query("INSERT INTO publish_jobs(id,generation,status,trigger_type) VALUES($1,2,'queued','manual')")
        .bind(ids::new_id()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE site_state SET desired_generation=2")
        .execute(&mut *tx)
        .await
        .unwrap();
    blocked(&mut tx).await;
    tx.rollback().await.unwrap();
    // SQLx 迁移失败时仍持有会话锁。
    connection.close().await.unwrap();

    for visible_member in [true, false] {
        check_series(pool, live_post, draft_revision, visible_member).await;
    }

    // 已在旧程序上成功切换的库已删除旧映射表。补装较早编号的迁移应当安全。
    let mut tx = pool.begin().await.unwrap();
    sqlx::migrate::Migrator {
        migrations: MIGRATOR
            .iter()
            .filter(|m| m.version != RECONCILE)
            .cloned()
            .collect(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&mut *tx)
    .await
    .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('site_generation_series') IS NULL")
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    );
    MIGRATOR.run(&mut *tx).await.unwrap();
    let state: i64 = sqlx::query_scalar("SELECT generation FROM site_state")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(state, 1);
    tx.rollback().await.unwrap();
}

async fn check_series(pool: &PgPool, live_post: Uuid, draft_revision: Uuid, visible_member: bool) {
    let mut connection = pool.acquire().await.unwrap();
    let mut tx = connection.begin().await.unwrap();
    let series = ids::new_id();
    let job = ids::new_id();
    let audit = ids::new_id();
    let member = ids::new_id();
    let member_draft = ids::new_id();
    sqlx::query("INSERT INTO series(id,slug,title,desired_published_at,first_published_at,slug_locked,publication_version)
        VALUES($1,'pending-series','待发布系列',now(),now(),true,1)")
        .bind(series).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO posts(id,slug,current_revision_id) VALUES($1,'series-draft',$2)")
        .bind(member)
        .bind(member_draft)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO post_revisions(id,post_id,revision_no,title,markdown_body) VALUES($1,$2,1,'目录内的草稿','保留正文')")
        .bind(member_draft).bind(member).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO series_items(id,series_id,post_id,position) VALUES($1,$2,$3,0)")
        .bind(ids::new_id())
        .bind(series)
        .bind(member)
        .execute(&mut *tx)
        .await
        .unwrap();
    if visible_member {
        sqlx::query("INSERT INTO series_items(id,series_id,post_id,position) VALUES($1,$2,$3,1)")
            .bind(ids::new_id())
            .bind(series)
            .bind(live_post)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO publish_jobs(id,generation,status,trigger_type,attempts,error_message,started_at,finished_at)
        VALUES($1,2,'failed','series.publish',3,'旧构建失败',now(),now())")
        .bind(job).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO audit_logs(id,action,resource_type,resource_id,metadata)
        VALUES($1,'series.publish','series',$2,jsonb_build_object('desired_generation',2,'publish_job_id',$3::uuid,'generation',1)),
              ($4,'publishing.request','publish_job',$3,'{}')")
        .bind(audit).bind(series).bind(job).bind(ids::new_id()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE site_state SET desired_generation=2")
        .execute(&mut *tx)
        .await
        .unwrap();
    if visible_member {
        blocked(&mut tx).await;
    } else {
        sqlx::migrate::Migrator {
            migrations: MIGRATOR
                .iter()
                .filter(|m| m.version < 20261128000000)
                .cloned()
                .collect(),
            ..sqlx::migrate::Migrator::DEFAULT
        }
        .run(&mut *tx)
        .await
        .unwrap();
    }
    let preserved: bool = sqlx::query_scalar("SELECT EXISTS (
        SELECT FROM series s JOIN series_items i ON i.series_id=s.id JOIN posts p ON p.id=i.post_id
        JOIN post_revisions r ON r.id=p.current_revision_id
        JOIN audit_logs a ON a.resource_id=s.id AND a.id=$2 AND a.metadata->'desired_generation'='2'::jsonb
        JOIN publish_jobs j ON j.id=$3 AND j.status='failed' AND j.error_message='旧构建失败'
        WHERE s.id=$1 AND s.desired_published_at IS NOT NULL AND s.publication_version=1
          AND p.id=$4 AND r.id=$5 AND r.markdown_body='保留正文' AND p.desired_published_revision_id IS NULL)")
        .bind(series).bind(audit).bind(job).bind(member).bind(member_draft)
        .fetch_one(&mut *tx).await.unwrap();
    assert!(preserved, "必须保留系列意图、草稿目录、失败任务与原始审计");
    let published: (Uuid, String) = sqlx::query_as(
        "SELECT p.current_revision_id,r.title FROM posts p JOIN published_posts r ON r.id=p.desired_published_revision_id WHERE p.id=$1")
        .bind(live_post).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(published, (draft_revision, "旧文章".to_owned()));
    if !visible_member {
        MIGRATOR.run(&mut *tx).await.unwrap();
        let (generation, published, draft): (i64, String, Uuid) = sqlx::query_as(
            "SELECT s.generation, r.title, p.current_revision_id FROM site_state s
             CROSS JOIN posts p JOIN published_posts r ON r.post_id=p.id WHERE p.id=$1",
        )
        .bind(live_post)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            (generation, published.as_str(), draft),
            (1, "旧文章", draft_revision)
        );
        let cleaned: bool = sqlx::query_scalar("SELECT metadata=jsonb_build_object('generation',1)
            AND NOT EXISTS (SELECT FROM audit_logs WHERE resource_type='publish_job' AND resource_id=$2)
            FROM audit_logs WHERE id=$1")
            .bind(audit).bind(job).fetch_one(&mut *tx).await.unwrap();
        assert!(cleaned, "清理任务及其引用，同时保留内容操作和公开版本");
        assert!(sqlx::query_scalar::<_, bool>("SELECT to_regclass('publication_history_archive') IS NULL AND to_regclass('outbox_events') IS NULL")
            .fetch_one(&mut *tx).await.unwrap());
    }
    tx.rollback().await.unwrap();
    connection.close().await.unwrap();
}
