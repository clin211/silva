//! 调用 Astro 构建公开站点。快照写入临时文件，按 `docs/contracts/site-snapshot.md` 的环境变量
//! 交给构建命令，构建成功后读取输出目录中的全部文件。
//!
//! Astro 7 用 `rename` 移动预渲染产物，输出目录必须与网站工程位于同一文件系统，
//! 因此临时目录建在网站工程的 `.astro/` 下。

use async_trait::async_trait;
use silva_application::ports::{BuiltFile, SiteBuilder};
use silva_shared::error::{AppError, Result};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

/// 构建进程组；离开作用域时向整个组发送 SIGKILL。构建正常结束后组内已无进程，信号无效果。
struct ProcessGroup(u32);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // 不使用 unsafe 调用 killpg，交给系统的 kill 命令；负数 PID 表示进程组。
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", self.0)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// 失败时保留的构建输出末尾字节数。
const OUTPUT_TAIL_BYTES: usize = 1500;

/// 构建命令及其运行环境。
#[derive(Clone, Debug)]
pub struct AstroSiteBuilder {
    /// `frontend/apps/website` 的路径。
    pub website_dir: PathBuf,
    /// 构建命令与参数，如 `["pnpm", "run", "build"]`。
    pub command: Vec<String>,
    /// 公开站点的完整 origin，传给 `SITE_URL`。
    pub site_url: String,
    pub timeout: Duration,
}

#[async_trait]
impl SiteBuilder for AstroSiteBuilder {
    #[tracing::instrument(name = "site.build", skip_all, fields(generation))]
    async fn build(
        &self,
        snapshot: &[u8],
        sha256: &str,
        generation: i64,
    ) -> Result<Vec<BuiltFile>> {
        let internal = |error: std::io::Error| AppError::Internal(error.into());
        let parent = self.website_dir.join(".astro");
        std::fs::create_dir_all(&parent).map_err(internal)?;
        let work = tempfile::Builder::new()
            .prefix("silva-build-")
            .tempdir_in(&parent)
            .map_err(internal)?;
        let snapshot_path = work.path().join("snapshot.json");
        let out_dir = work.path().join("dist");
        std::fs::write(&snapshot_path, snapshot).map_err(internal)?;
        let (program, args) = self
            .command
            .split_first()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("site build command is empty")))?;
        let child = tokio::process::Command::new(program)
            .args(args)
            .current_dir(&self.website_dir)
            .env("SILVA_SNAPSHOT", &snapshot_path)
            .env("SILVA_SNAPSHOT_SHA256", sha256)
            .env("SILVA_SNAPSHOT_GENERATION", generation.to_string())
            .env("SILVA_SITE_OUT_DIR", &out_dir)
            .env("SITE_URL", &self.site_url)
            // 构建在隔离环境中运行，不向外发送遥测。
            .env("ASTRO_TELEMETRY_DISABLED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // 独立进程组：超时或放弃构建时连同 pnpm 启动的 node 子进程一起结束。
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                AppError::Internal(anyhow::anyhow!("cannot start site build: {error}"))
            })?;
        let _group = child.id().map(ProcessGroup);
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                AppError::Internal(anyhow::anyhow!(
                    "site build timed out after {} seconds",
                    self.timeout.as_secs()
                ))
            })?
            .map_err(internal)?;
        if !output.status.success() {
            let mut log = output.stdout;
            log.extend_from_slice(&output.stderr);
            return Err(AppError::Internal(anyhow::anyhow!(
                "site build exited with {}: {}",
                output.status,
                tail(&log)
            )));
        }
        let files = tokio::task::spawn_blocking(move || collect(&out_dir))
            .await
            .map_err(|error| AppError::Internal(error.into()))?
            .map_err(internal)?;
        drop(work);
        Ok(files)
    }
}

fn tail(log: &[u8]) -> String {
    let text = String::from_utf8_lossy(log);
    let start = text.len().saturating_sub(OUTPUT_TAIL_BYTES);
    let start = (start..text.len())
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(text.len());
    text[start..].trim().to_owned()
}

/// 递归读取输出目录中的文件，路径统一为 `/` 分隔的相对路径。
fn collect(root: &Path) -> std::io::Result<Vec<BuiltFile>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(std::io::Error::other)?
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                files.push(BuiltFile {
                    content_type: content_type(&relative),
                    bytes: std::fs::read(&path)?,
                    path: relative,
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// 按扩展名推断内容类型，文本类型显式声明 UTF-8。
pub fn content_type(path: &str) -> String {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let essence = mime.essence_str();
    let textual = mime.type_() == mime_guess::mime::TEXT
        || matches!(
            essence,
            "application/json" | "application/xml" | "application/javascript" | "image/svg+xml"
        );
    if textual {
        format!("{essence}; charset=utf-8")
    } else {
        essence.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_declare_utf8_for_text() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("rss.xml"), "text/xml; charset=utf-8");
        assert_eq!(
            content_type("_astro/app.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type("hero.webp"), "image/webp");
        assert_eq!(content_type("unknown.bin"), "application/octet-stream");
    }

    #[tokio::test]
    async fn runs_command_with_snapshot_environment_and_collects_output() {
        let website = tempfile::tempdir().unwrap();
        // 用 shell 模拟构建：校验环境变量并写出产物。
        let script = r#"set -e
test "$(sha256sum "$SILVA_SNAPSHOT" | cut -d' ' -f1)" = "$SILVA_SNAPSHOT_SHA256"
test "$SILVA_SNAPSHOT_GENERATION" = 9
test "$SITE_URL" = https://example.test
mkdir -p "$SILVA_SITE_OUT_DIR/blog/a"
echo home > "$SILVA_SITE_OUT_DIR/index.html"
echo post > "$SILVA_SITE_OUT_DIR/blog/a/index.html""#;
        let builder = AstroSiteBuilder {
            website_dir: website.path().to_path_buf(),
            command: vec!["sh".into(), "-c".into(), script.into()],
            site_url: "https://example.test".into(),
            timeout: Duration::from_secs(30),
        };
        let snapshot = br#"{"generation":9}"#;
        let sha = silva_application::publishing::sha256_hex(snapshot);
        let files = builder.build(snapshot, &sha, 9).await.unwrap();
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["blog/a/index.html", "index.html"]
        );
        assert_eq!(files[1].bytes, b"home\n");
        // 临时目录在构建后删除。
        let leftovers = std::fs::read_dir(website.path().join(".astro"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 0);

        let failing = AstroSiteBuilder {
            command: vec!["sh".into(), "-c".into(), "echo boom >&2; exit 3".into()],
            ..builder.clone()
        };
        let error = failing
            .build(snapshot, &sha, 9)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("boom") && error.contains("3"), "{error}");

        // 超时后整个进程组被结束，包括构建命令启动的后台子进程。
        let marker = website.path().join("child.pid");
        let slow = AstroSiteBuilder {
            command: vec![
                "sh".into(),
                "-c".into(),
                format!("sleep 30 & echo $! > {}; wait", marker.display()),
            ],
            timeout: Duration::from_millis(500),
            ..builder
        };
        let error = slow.build(snapshot, &sha, 9).await.unwrap_err().to_string();
        assert!(error.contains("timed out"), "{error}");
        let pid = std::fs::read_to_string(&marker).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !Path::new(&format!("/proc/{}", pid.trim())).exists(),
            "构建启动的子进程应随进程组结束"
        );
    }
}
