//! 复用文章验收的认证与存储，验证系列实际路由。
use super::*;
use uuid::Uuid;

#[path = "publication_cutover.rs"]
mod publication_cutover;

const SERIES: &str = "/api/v1/admin/series";

pub(super) async fn before_upgrade(pool: &PgPool) {
    use silva_infrastructure::persistence::MIGRATOR;
    let migrations = MIGRATOR
        .iter()
        .take_while(|m| m.version < 20261126000000)
        .cloned()
        .collect();
    sqlx::migrate::Migrator {
        migrations,
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let post = ids::new_id();
    let revision = ids::new_id();
    sqlx::query("INSERT INTO posts(id,slug,current_revision_id,desired_published_revision_id,desired_published_at,first_published_at,slug_locked)
        VALUES($1,'legacy',$2,$2,now(),now(),true)").bind(post).bind(revision).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO post_revisions(id,post_id,revision_no,title,markdown_body) VALUES($1,$2,1,'旧文章','')").bind(revision).bind(post).execute(&mut *tx).await.unwrap();
    let draft = ids::new_id();
    sqlx::query("INSERT INTO post_revisions(id,post_id,revision_no,title,markdown_body) VALUES($1,$2,2,'新草稿','')").bind(draft).bind(post).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE posts SET current_revision_id=$1 WHERE id=$2")
        .bind(draft)
        .bind(post)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publish_jobs(id,generation,status,trigger_type,manifest_key,artifact_prefix,started_at,finished_at) VALUES($1,1,'active','bootstrap','legacy/manifest.json','legacy/',now(),now())").bind(ids::new_id()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO site_generation_posts VALUES(1,$1,$2)")
        .bind(post)
        .bind(revision)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE site_state SET active_generation=1,desired_generation=1,activated_at=now()",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO media_assets(id,upload_key,original_filename,declared_mime_type,declared_size_bytes,upload_expires_at) VALUES($1,'uploads/legacy','legacy.png','image/png',1,now())")
        .bind(ids::new_id()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    publication_cutover::check_original_schema(pool, post, draft).await;

    // 模拟最后一次旧发布尚在排队的库。切换应在修改任何线上引用之前回滚。
    let legacy = sqlx::migrate::Migrator {
        migrations: MIGRATOR
            .iter()
            .take_while(|m| m.version < 20261127000000)
            .cloned()
            .collect(),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy.run(pool).await.unwrap();
    publication_cutover::check(pool, post, draft).await;
}

async fn write(
    app: &Router,
    method: Method,
    path: &str,
    body: Value,
    token: &str,
    status: StatusCode,
) -> Value {
    let r = api(app, method, path, Some(body), token).await;
    assert_eq!(r.status, status, "{path} {}", r.json);
    r.json["data"].clone()
}

async fn draft_posts(pool: &PgPool) -> Vec<Uuid> {
    let posts: Vec<_> = (0..201).map(|_| ids::new_id()).collect();
    let revisions: Vec<_> = posts.iter().map(|_| ids::new_id()).collect();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO posts(id,slug,current_revision_id,author_id) SELECT p,'draft-'||p,r,(SELECT id FROM users WHERE email='author@example.test')
        FROM unnest($1::uuid[],$2::uuid[]) x(p,r)").bind(&posts).bind(&revisions).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO post_revisions(id,post_id,revision_no,title,markdown_body) SELECT r,p,1,'初稿','' FROM unnest($1::uuid[],$2::uuid[]) x(p,r)")
        .bind(&posts).bind(&revisions).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    posts
}

async fn generation(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT generation FROM site_state WHERE singleton")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn race(
    app: &Router,
    editor: &str,
    a: (&str, Method, Value),
    b: (&str, Method, Value),
    code: &str,
) {
    let (one, two) = tokio::join!(
        api(app, a.1, a.0, Some(a.2), editor),
        api(app, b.1, b.0, Some(b.2), editor)
    );
    let (ok, conflict) = if one.status.is_success() {
        (one, two)
    } else {
        (two, one)
    };
    assert!(ok.status.is_success(), "{}", ok.json);
    assert_error(&conflict, StatusCode::CONFLICT, code);
}

async fn editing(app: &Router, pool: &PgPool, editor: &str, posts: &[Uuid]) {
    let create = async |slug: &str| {
        let row = write(
            app,
            Method::POST,
            SERIES,
            json!({"title":"并发","slug":slug}),
            editor,
            StatusCode::CREATED,
        )
        .await;
        format!("{SERIES}/{}", row["id"].as_str().unwrap())
    };
    let a = create("race-one").await;
    let b = create("race-two").await;
    let path = format!("{a}/outline");
    let items = format!("{a}/items");
    let other = format!("{b}/items");
    let req = |v, p| json!({"expected_edit_version":v,"post_id":p});
    race(
        app,
        editor,
        (&items, Method::POST, req(0, posts[0])),
        (&other, Method::POST, req(0, posts[0])),
        "post_already_in_series",
    )
    .await;
    let member_path = format!("/api/v1/admin/posts/{}", posts[0]);
    let member = get(app, &member_path, editor).await.json;
    let revision = member["data"]["current_revision_id"].as_str().unwrap();
    let version = member["data"]["publication"]["publication_version"]
        .as_i64()
        .unwrap();
    assert_error(
        &api(app, Method::DELETE, &format!("{member_path}?expected_revision_id={revision}&expected_publication_version={version}"), None, editor).await,
        StatusCode::CONFLICT, "post_already_in_series",
    );
    let v = get(app, &a, editor).await.json["data"]["edit_version"]
        .as_i64()
        .unwrap();
    race(
        app,
        editor,
        (
            &a,
            Method::PATCH,
            json!({"expected_edit_version":v,"title":"甲"}),
        ),
        (
            &a,
            Method::PATCH,
            json!({"expected_edit_version":v,"title":"乙"}),
        ),
        "series_edit_conflict",
    )
    .await;
    race(
        app,
        editor,
        (
            &path,
            Method::PUT,
            json!({"expected_edit_version":v+1,"sections":[],"items":[{"post_id":posts[2]}]}),
        ),
        (&items, Method::POST, req(v + 1, posts[1])),
        "series_edit_conflict",
    )
    .await;
    // 清理占用的文章后校验多字节目录的完整容量。
    for resource in [&a, &b] {
        let version = &get(app, resource, editor).await.json["data"]["edit_version"];
        let url =
            format!("{resource}?expected_edit_version={version}&expected_publication_version=0");
        assert_eq!(
            api(app, Method::DELETE, &url, None, editor).await.status,
            StatusCode::NO_CONTENT
        );
    }
    let full = create("capacity").await;
    let path = format!("{full}/outline");
    let groups: Vec<_> = (0..50)
        .map(|i| json!({"title":format!("第 {i} 章"),"items":[]}))
        .collect();
    let items: Vec<_> = posts
        .iter()
        .take(200)
        .map(|id| json!({"post_id":id,"display_title":"章".repeat(200)}))
        .collect();
    let body = json!({"expected_edit_version":0,"items":items,"sections":groups});
    assert!(body.to_string().len() > 64 * 1024);
    let saved = write(
        app,
        Method::PUT,
        &path,
        body.clone(),
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved["series"]["item_count"], 200);
    let count = generation(pool).await;
    let mut repeat = body;
    repeat["expected_edit_version"] = json!(1);
    for (input, s) in repeat["sections"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(saved["series"]["sections"].as_array().unwrap())
    {
        input["id"] = s["id"].clone();
    }
    assert_eq!(
        write(
            app,
            Method::PUT,
            &path,
            repeat.clone(),
            editor,
            StatusCode::OK
        )
        .await,
        saved
    );
    let mut invalid = repeat.clone();
    invalid["sections"]
        .as_array_mut()
        .unwrap()
        .push(json!({"title":"多余","items":[]}));
    assert_field(
        &api(app, Method::PUT, &path, Some(invalid), editor).await,
        "sections",
        "out_of_range",
    );
    repeat["items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"post_id":posts[200]}));
    assert_field(
        &api(app, Method::PUT, &path, Some(repeat), editor).await,
        "items",
        "out_of_range",
    );
    assert_field(
        &api(
            app,
            Method::POST,
            &format!("{full}/items"),
            Some(req(1, posts[200])),
            editor,
        )
        .await,
        "post_id",
        "out_of_range",
    );
    let huge = json!({"expected_edit_version":1,"sections":[],"items":[{"post_id":posts[0],"display_title":"长".repeat(350000)}]});
    assert_error(
        &api(app, Method::PUT, &path, Some(huge), editor).await,
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
    );
    assert_eq!(generation(pool).await, count);
    assert_eq!(
        api(
            app,
            Method::DELETE,
            &format!("{full}?expected_edit_version=1&expected_publication_version=0"),
            None,
            editor
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
}

async fn active(app: &Router) -> SiteSnapshot {
    let response = call(
        app,
        Method::GET,
        "/api/v1/public/site-snapshot",
        None,
        None,
        &[],
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.json);
    serde_json::from_value(response.json["data"].clone()).unwrap()
}

pub(super) async fn run(pool: &PgPool, app: &Router, editor: &str, author: &str) {
    silva_infrastructure::persistence::migrate(pool)
        .await
        .unwrap();
    let permissions:Vec<String>=sqlx::query_scalar("SELECT r.key FROM roles r JOIN role_permissions rp ON rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE p.key='series.manage' ORDER BY r.key")
        .fetch_all(pool).await.unwrap();
    assert_eq!(permissions, ["admin", "editor"]);
    assert_eq!(
        get(app, SERIES, editor).await.json["meta"]["pagination"]["total"],
        0
    );
    assert_error(
        &call(app, Method::GET, SERIES, None, None, &[]).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    );
    assert_error(
        &get(app, SERIES, author).await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );
    let legacy: Uuid = sqlx::query_scalar("SELECT id FROM posts WHERE slug='legacy'")
        .fetch_one(pool)
        .await
        .unwrap();
    // 升级后留下最后一次编辑的草稿，旧线上文章仍按原先的发布目标构建。
    let migrated = get(app, &format!("/api/v1/admin/posts/{legacy}"), editor).await;
    assert_eq!(migrated.json["data"]["current_revision"]["title"], "新草稿");
    assert_eq!(
        get(
            app,
            &format!("/api/v1/admin/posts/{legacy}/revisions"),
            editor
        )
        .await
        .json["meta"]["pagination"]["total"],
        1
    );
    let posts = draft_posts(pool).await;
    editing(app, pool, editor, &posts).await;
    let category = write(
        app,
        Method::POST,
        "/api/v1/admin/categories",
        json!({"slug":"series-only","name":"系列主分类"}),
        editor,
        StatusCode::CREATED,
    )
    .await;
    let category_path = format!(
        "/api/v1/admin/categories/{}",
        category["id"].as_str().unwrap()
    );
    let cover = ready_media(app, author, 107).await;
    let media = format!("/api/v1/admin/media/{cover}");
    let data=write(app,Method::POST,SERIES,json!({"slug":"guide","title":" 指南 ","category_id":category["id"],"cover_media_id":cover}),editor,StatusCode::CREATED).await;
    let id = data["id"].as_str().unwrap();
    let path = format!("{SERIES}/{id}");
    let outline = format!("{path}/outline");
    let publication = format!("{path}/publication");
    assert_eq!(data["title"], "指南");
    assert_error(
        &api(app, Method::DELETE, &category_path, None, editor).await,
        StatusCode::CONFLICT,
        "taxonomy_in_use",
    );
    assert_eq!(
        get(app, &media, author).await.json["meta"]["references"],
        json!([])
    );
    assert_eq!(
        get(app, &media, author).await.json["meta"]["reference_count"],
        1
    );
    let publish = |v| json!({"expected_publication_version":v});
    write(
        app,
        Method::PUT,
        &publication,
        publish(0),
        editor,
        StatusCode::OK,
    )
    .await;
    assert!(active(app).await.series.is_empty());
    assert_error(
        &api(app, Method::PUT, &publication, Some(publish(0)), editor).await,
        StatusCode::CONFLICT,
        "publication_conflict",
    );
    assert_error(
        &api(app, Method::PUT, &publication, Some(publish(1)), editor).await,
        StatusCode::CONFLICT,
        "already_published",
    );
    assert_error(
        &api(
            app,
            Method::PATCH,
            &path,
            Some(json!({"expected_edit_version":0,"slug":"locked"})),
            editor,
        )
        .await,
        StatusCode::CONFLICT,
        "slug_locked",
    );

    let count = generation(pool).await;
    let input = json!({"expected_edit_version":0,"items":[{"post_id":legacy}],"sections":[
        {"title":"下一章","items":[{"post_id":posts[0],"display_title":"下一篇"}]},
        {"title":"草稿","items":[{"post_id":posts[1]}]}]});
    let saved = write(
        app,
        Method::PUT,
        &outline,
        input.clone(),
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved["series"]["published_post_count"], 1);
    assert_eq!(saved["series"]["item_count"], 3);
    let mut same = input;
    same["expected_edit_version"] = json!(1);
    for i in 0..2 {
        same["sections"][i]["id"] = saved["series"]["sections"][i]["id"].clone();
    }
    assert!(
        write(
            app,
            Method::PUT,
            &outline,
            same.clone(),
            editor,
            StatusCode::OK
        )
        .await["publication"]
            .is_null()
    );
    let mut foreign = same.clone();
    foreign["sections"][1]["id"] = json!(ids::new_id());
    assert_field(
        &api(app, Method::PUT, &outline, Some(foreign), editor).await,
        "sections[1].id",
        "invalid_value",
    );
    let mut duplicated = same.clone();
    duplicated["sections"][0]["items"][0]["post_id"] = json!(legacy);
    assert_field(
        &api(app, Method::PUT, &outline, Some(duplicated), editor).await,
        "sections[0].items[0].post_id",
        "duplicate",
    );
    let mut bad = same.clone();
    bad["items"][0]["post_id"] = json!(ids::new_id());
    assert_error(
        &api(app, Method::PUT, &outline, Some(bad), editor).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    assert_eq!(
        get(app, &path, editor).await.json["data"]["edit_version"],
        1
    );
    assert_eq!(generation(pool).await, count + 1);
    let snap = active(app).await;
    assert_eq!(snap.series[0].items[0].post_slug, "legacy");
    assert_eq!(
        snap.series[0].items[0].display_title.as_deref(),
        Some("旧文章")
    );
    assert!(snap.series[0].sections.is_empty() && snap.categories.is_empty());
    assert_eq!(
        snap.series[0].category.as_ref().unwrap().slug,
        "series-only"
    );
    assert_eq!(snap.media.len(), 1);
    let mut damaged = snap.clone();
    damaged.media.clear();
    assert!(damaged.validate_series().is_err());
    let mut damaged = snap.clone();
    damaged.series[0].items[0].post_slug = "missing".into();
    assert!(damaged.validate_series().is_err());
    let mut duplicated = snap.clone();
    duplicated.series[0]
        .items
        .push(snap.series[0].items[0].clone());
    assert!(duplicated.validate_series().is_err());
    let series_list = format!(
        "{SERIES}?q=guid&category_id={}&published=true&completion_status=ongoing&sort=updated_desc",
        category["id"].as_str().unwrap()
    );
    assert_eq!(
        get(app, &series_list, editor).await.json["data"][0]["edit_version"],
        1
    );
    assert_eq!(
        get(app, &format!("/api/v1/admin/posts/{}", posts[0]), author)
            .await
            .json["data"]["series"],
        Value::Null
    );
    for endpoint in ["posts", "posts/category-counts"] {
        let query = format!("/api/v1/admin/{endpoint}?series_id={id}");
        assert_error(
            &get(app, &query, author).await,
            StatusCode::FORBIDDEN,
            "forbidden",
        );
        assert_field(
            &get(app, &format!("{query}&in_series=false"), editor).await,
            "in_series",
            "invalid_value",
        );
        let result = get(app, &query, editor).await;
        if endpoint == "posts" {
            assert_eq!(result.json["meta"]["pagination"]["total"], 3);
            assert_eq!(result.json["data"][0]["series"]["id"], id);
        } else {
            assert_eq!(result.json["data"]["total"], 3);
        }
    }
    let count = generation(pool).await;
    write(
        app,
        Method::PATCH,
        &category_path,
        json!({"description":"新简介"}),
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(generation(pool).await, count + 1);
    assert_eq!(
        get(app, &path, editor).await.json["data"]["edit_version"],
        1
    );
    // 一篇草稿直接发布后，同一篇文章的构建会重算公开目录。
    let post_path = format!("/api/v1/admin/posts/{}", posts[0]);
    let revision = get(app, &post_path, editor).await.json["data"]["current_revision_id"].clone();
    let fresh = ready_media(app, editor, 108).await;
    let revised=write(app,Method::POST,&format!("{post_path}/revisions"),
        json!({"expected_revision_id":revision,"title":"准备","markdown_body":"","cover_media_id":fresh}),editor,StatusCode::CREATED).await;
    let revision = revised["id"].clone();
    write(
        app,
        Method::PUT,
        &format!("{post_path}/publication"),
        json!({"revision_id":revision,"expected_publication_version":0}),
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        get(app, &path, editor).await.json["data"]["edit_version"],
        1
    );
    let snap = active(app).await;
    assert_eq!(snap.series[0].sections.len(), 1);
    assert_eq!(
        snap.series[0].sections[0].items[0].display_title.as_deref(),
        Some("下一篇")
    );
    assert_eq!(
        snap.series[0].updated_at,
        snap.posts.iter().map(|p| p.updated_at).max().unwrap()
    );

    // 从篇章移到根，并把整个篇章删除，文章本身始终保留。
    let moved = json!({"expected_edit_version":1,"items":[{"post_id":posts[0]},{"post_id":legacy}],"sections":[]});
    write(app, Method::PUT, &outline, moved, editor, StatusCode::OK).await;
    assert_eq!(
        get(app, &post_path, editor).await.json["data"]["series"]["position"],
        0
    );
    assert!(get(app, &post_path, editor).await.json["data"]["series"]["section_title"].is_null());

    write(
        app,
        Method::PATCH,
        &path,
        json!({"expected_edit_version":2,"cover_media_id":fresh,"description":"资料"}),
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        get(app, &path, editor).await.json["data"]["edit_version"],
        3
    );
    assert_error(
        &get(app, &format!("/api/v1/public/media/{cover}"), editor).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    let previous = active(app).await.generation;
    let media_path = format!("/api/v1/admin/media/{fresh}");
    let updated_alt = json!({"alt_text":"文章和系列共用的封面"});
    write(
        app,
        Method::PATCH,
        &media_path,
        updated_alt.clone(),
        editor,
        StatusCode::OK,
    )
    .await;
    let shared = active(app).await;
    assert_eq!(shared.generation, previous + 1);
    assert_eq!(
        shared.series[0].cover.as_ref().unwrap().alt.as_deref(),
        Some("文章和系列共用的封面")
    );
    write(
        app,
        Method::PATCH,
        &media_path,
        updated_alt,
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(active(app).await.generation, shared.generation);
    assert_eq!(
        shared
            .media
            .iter()
            .filter(|m| m.id.to_string() == fresh)
            .count(),
        1
    );
    assert_eq!(
        shared.series[0]
            .cover
            .as_ref()
            .unwrap()
            .media_id
            .to_string(),
        fresh
    );
    assert_eq!(
        api(app, Method::DELETE, &media, None, editor).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        write(
            app,
            Method::PATCH,
            &path,
            json!({"expected_edit_version":3,"description":null}),
            editor,
            StatusCode::OK
        )
        .await["series"]["description"],
        Value::Null
    );
    assert_eq!(
        api(
            app,
            Method::DELETE,
            &format!("{path}/items/{}?expected_edit_version=4", posts[0]),
            None,
            editor
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api(
            app,
            Method::DELETE,
            &format!("{post_path}/publication?expected_publication_version=1"),
            None,
            editor
        )
        .await
        .status,
        StatusCode::OK
    );
    let after_post = active(app).await;
    assert_eq!(after_post.posts.len(), 1);
    assert_eq!(after_post.series[0].items.len(), 1);
    let unchanged = write(
        app,
        Method::PUT,
        &outline,
        json!({"expected_edit_version":5,"sections":[],"items":[{"post_id":legacy}]}),
        editor,
        StatusCode::OK,
    )
    .await;
    assert_eq!(unchanged["series"]["edit_version"], 5);
    assert_eq!(unchanged["publication"], Value::Null);
    let off = format!("{publication}?expected_publication_version=1");
    assert_eq!(
        api(app, Method::DELETE, &off, None, editor).await.status,
        StatusCode::OK
    );
    let deletion = format!("{path}?expected_edit_version=5&expected_publication_version=2");
    assert_eq!(get(app, &path, editor).await.json["data"]["is_live"], false);
    assert_error(
        &api(
            app,
            Method::DELETE,
            &format!("{publication}?expected_publication_version=2"),
            None,
            editor,
        )
        .await,
        StatusCode::CONFLICT,
        "already_unpublished",
    );
    assert!(active(app).await.series.is_empty());
    assert_eq!(
        get(app, "/api/v1/public/posts/legacy", editor).await.status,
        StatusCode::OK
    );
    assert_eq!(
        api(app, Method::DELETE, &deletion, None, editor)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(app, &format!("/api/v1/admin/posts/{legacy}"), editor)
            .await
            .status,
        StatusCode::OK
    );
}
