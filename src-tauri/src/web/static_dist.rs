//! Web 前端静态资源的两种来源：
//!
//! 1. 磁盘目录（`--dist` / `CC_SWITCH_WEB_DIST`，默认 `dist-web`）：开发、热更新前端用，
//!    存在 `index.html` 就优先用它；
//! 2. 内嵌产物（`rust-embed`，release 构建把 `../dist-web` 编译期打进二进制）：
//!    单个二进制拷到任何机器直接 `--web` 启动即可访问，不需要带前端目录。
//!
//! 内嵌只影响 `--features web` 构建；桌面构建不编译本模块。

use std::path::{Path, PathBuf};

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// 编译期嵌入的 `dist-web` 目录（相对 `src-tauri/`）。debug 构建按需读磁盘（rust-embed 默认行为），
/// release 构建直接进二进制（gzip 压缩）。目录不存在时是空集合，不报错。
#[cfg(feature = "web")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../dist-web/"]
struct EmbeddedDist;

#[derive(Clone)]
pub struct StaticSource {
    /// 磁盘前端目录；`index.html` 缺失时视为没有（例如只带了二进制部署）
    disk_dir: Option<PathBuf>,
}

impl StaticSource {
    pub fn resolve(dist_dir: &Path) -> Self {
        let disk_dir = dist_dir.join("index.html").is_file().then(|| dist_dir.to_path_buf());
        Self { disk_dir }
    }

    pub fn has_disk(&self) -> bool {
        self.disk_dir.is_some()
    }

    /// 读取一个静态文件：先磁盘后内嵌。`None` = 两边都没有。
    pub fn read(&self, path: &str) -> Option<(Vec<u8>, &'static str)> {
        if let Some(dir) = &self.disk_dir {
            let clean = sanitize(path);
            let file = dir.join(&clean);
            if file.is_file() {
                if let Ok(bytes) = std::fs::read(&file) {
                    return Some((bytes, mime_of(&clean)));
                }
            }
        }
        self.read_embedded(path)
    }

    fn read_embedded(&self, path: &str) -> Option<(Vec<u8>, &'static str)> {
        #[cfg(feature = "web")]
        {
            let clean = sanitize(path);
            match EmbeddedDist::get(&clean) {
                Some(file) => {
                    let bytes = file.data.to_vec();
                    Some((bytes, mime_of(&clean)))
                }
                None => {
                    log::debug!("内嵌资源未命中: {clean:?}（内嵌条目数 {}）", EmbeddedDist::iter().count());
                    None
                }
            }
        }
        #[cfg(not(feature = "web"))]
        {
            let _ = path;
            None
        }
    }

    /// SPA 入口：磁盘 `index.html`，没有就内嵌的。
    pub fn index(&self) -> Option<(Vec<u8>, &'static str)> {
        self.read("index.html")
    }
}

/// 去掉 `..` / 绝对路径前缀等，防目录穿越；内嵌侧 rust-embed 本身也只匹配白名单路径。
fn sanitize(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    Path::new(trimmed)
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_string()
}

fn mime_of(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

pub fn static_response(bytes: Vec<u8>, mime: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    (StatusCode::OK, headers, bytes).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal() {
        assert_eq!(sanitize("../../etc/passwd"), "etc/passwd");
        assert_eq!(sanitize("/../../etc/passwd"), "etc/passwd");
        assert_eq!(sanitize("assets/main.js"), "assets/main.js");
    }

    #[test]
    fn mime_covers_main_types() {
        assert_eq!(mime_of("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_of("a/b/c.js"), "text/javascript; charset=utf-8");
        assert_eq!(mime_of("font.woff2"), "font/woff2");
    }
}

#[cfg(test)]
mod embed_tests {
    use super::*;

    #[test]
    fn embedded_index_exists() {
        let file = EmbeddedDist::get("index.html");
        assert!(file.is_some(), "index.html 未嵌入：构建时 dist-web 目录不存在或为空");
    }

    #[test]
    fn embedded_assets_readable() {
        let some = EmbeddedDist::iter().next();
        println!("embedded sample: {some:?}");
        assert!(EmbeddedDist::iter().count() > 0, "内嵌集合为空");
    }

    /// 复现运行时 lookup：sanitize 后的路径必须能在 EmbeddedDist 里命中
    #[test]
    fn embedded_asset_via_sanitize() {
        let cleaned = sanitize("assets/main-Cgh3ayM3.js");
        println!("sanitized: {cleaned:?}");
        let hit = EmbeddedDist::get(&cleaned);
        println!("hit: {}", hit.is_some());
        let names: Vec<_> = EmbeddedDist::iter().take(5).collect();
        println!("first entries: {names:?}");
        assert!(hit.is_some(), "sanitize 后的路径 {cleaned:?} 未命中内嵌集合");
    }
}
