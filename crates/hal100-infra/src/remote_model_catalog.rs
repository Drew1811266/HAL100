use std::time::Duration;

use bytes::BytesMut;
use futures_util::StreamExt;
use hal100_protocol::{
    DownloadSource, RemoteGgufFile, RemoteModelRepository, RemoteModelSearchItem,
    RemoteModelSearchResults,
};
use reqwest::{Client, StatusCode, Url, redirect::Policy};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

use crate::model_import::quantization_from_file_name;

const MAX_CATALOG_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 20;
const MAX_REPOSITORY_FILES: usize = 5_000;

#[derive(Debug, Error)]
pub enum RemoteModelCatalogError {
    #[error("搜索词长度必须为 2—100 个字符")]
    InvalidQuery,
    #[error("模型仓库必须使用 owner/name 格式")]
    InvalidRepository,
    #[error("远端模型服务返回 HTTP {status}")]
    UpstreamStatus { status: u16 },
    #[error("远端模型服务响应超过 4 MiB 安全上限")]
    ResponseTooLarge,
    #[error("远端模型服务返回了无法识别的数据")]
    InvalidResponse,
    #[error("远端模型服务暂时不可用：{0}")]
    Network(String),
    #[error("模型仓库没有可用的 GGUF 文件")]
    NoGgufFiles,
    #[error("HAL100 远端模型端点配置无效")]
    InvalidEndpoint,
}

#[derive(Clone)]
pub struct RemoteModelCatalog {
    client: Client,
    endpoints: CatalogEndpoints,
}

#[derive(Clone)]
struct CatalogEndpoints {
    hugging_face_api: Url,
    model_scope_openapi: Url,
    model_scope_legacy_api: Url,
}

impl RemoteModelCatalog {
    pub fn new() -> Result<Self, RemoteModelCatalogError> {
        Self::with_endpoints(
            "https://huggingface.co/api/",
            "https://modelscope.cn/openapi/v1/",
            "https://modelscope.cn/api/v1/",
        )
    }

    pub(crate) fn with_endpoints(
        hugging_face_api: &str,
        model_scope_openapi: &str,
        model_scope_legacy_api: &str,
    ) -> Result<Self, RemoteModelCatalogError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(Policy::limited(3))
            .user_agent("HAL100/0.0.1-dev model-catalog")
            .build()
            .map_err(|error| RemoteModelCatalogError::Network(error.to_string()))?;
        Ok(Self {
            client,
            endpoints: CatalogEndpoints {
                hugging_face_api: parse_base_url(hugging_face_api)?,
                model_scope_openapi: parse_base_url(model_scope_openapi)?,
                model_scope_legacy_api: parse_base_url(model_scope_legacy_api)?,
            },
        })
    }

    pub async fn search(
        &self,
        source: DownloadSource,
        query: &str,
    ) -> Result<RemoteModelSearchResults, RemoteModelCatalogError> {
        let query = validate_query(query)?;
        let items = match source {
            DownloadSource::HuggingFace => self.search_hugging_face(&query).await?,
            DownloadSource::ModelScope => self.search_model_scope(&query).await?,
        };
        Ok(RemoteModelSearchResults {
            source,
            query,
            items,
        })
    }

    pub async fn repository(
        &self,
        source: DownloadSource,
        repository: &str,
    ) -> Result<RemoteModelRepository, RemoteModelCatalogError> {
        let (owner, name) = validate_repository(repository)?;
        match source {
            DownloadSource::HuggingFace => self.hugging_face_repository(owner, name).await,
            DownloadSource::ModelScope => self.model_scope_repository(owner, name).await,
        }
    }

    async fn search_hugging_face(
        &self,
        query: &str,
    ) -> Result<Vec<RemoteModelSearchItem>, RemoteModelCatalogError> {
        let limit = MAX_SEARCH_RESULTS.to_string();
        let url = self
            .endpoints
            .hugging_face_api
            .join("models")
            .map_err(|_| RemoteModelCatalogError::InvalidEndpoint)?;
        let response: Vec<HuggingFaceModel> = self
            .get_json(self.client.get(url).query(&[
                ("search", query),
                ("filter", "gguf"),
                ("limit", limit.as_str()),
                ("sort", "downloads"),
                ("direction", "-1"),
                ("full", "true"),
            ]))
            .await?;
        Ok(response
            .into_iter()
            .filter_map(|model| search_item_from_hugging_face(model).ok())
            .take(MAX_SEARCH_RESULTS)
            .collect())
    }

    async fn search_model_scope(
        &self,
        query: &str,
    ) -> Result<Vec<RemoteModelSearchItem>, RemoteModelCatalogError> {
        let page_size = MAX_SEARCH_RESULTS.to_string();
        let url = self
            .endpoints
            .model_scope_openapi
            .join("models")
            .map_err(|_| RemoteModelCatalogError::InvalidEndpoint)?;
        let response: ModelScopeSearchEnvelope = self
            .get_json(self.client.get(url).query(&[
                ("search", query),
                ("page_number", "1"),
                ("page_size", page_size.as_str()),
            ]))
            .await?;
        ensure_model_scope_success(response.success)?;
        Ok(response
            .data
            .models
            .into_iter()
            .filter(|model| {
                model
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case("library:gguf"))
            })
            .filter_map(|model| search_item_from_model_scope(model).ok())
            .take(MAX_SEARCH_RESULTS)
            .collect())
    }

    async fn hugging_face_repository(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<RemoteModelRepository, RemoteModelCatalogError> {
        let url = repository_url(&self.endpoints.hugging_face_api, "models", owner, name)?;
        let model: HuggingFaceModel = self
            .get_json(self.client.get(url).query(&[("blobs", "true")]))
            .await?;
        let repository = validate_repository(&model.id)
            .map(|_| model.id.clone())
            .unwrap_or_else(|_| format!("{owner}/{name}"));
        let revision = model
            .sha
            .clone()
            .ok_or(RemoteModelCatalogError::InvalidResponse)?;
        let mut files = model
            .siblings
            .into_iter()
            .take(MAX_REPOSITORY_FILES)
            .filter_map(|file| {
                remote_hugging_face_file(file, &revision)
                    .filter(|file| safe_remote_file(&file.path))
            })
            .collect::<Vec<_>>();
        normalize_files(&mut files)?;
        Ok(RemoteModelRepository {
            source: DownloadSource::HuggingFace,
            display_name: repository
                .split_once('/')
                .map(|(_, name)| name.to_owned())
                .unwrap_or_else(|| repository.clone()),
            repository,
            license: license_from_hugging_face(&model.tags, model.card_data.as_ref()),
            gated: truthy(&model.gated),
            private: model.private,
            files,
        })
    }

    async fn model_scope_repository(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<RemoteModelRepository, RemoteModelCatalogError> {
        let detail_url =
            repository_url(&self.endpoints.model_scope_openapi, "models", owner, name)?;
        let detail: ModelScopeDetailEnvelope = self.get_json(self.client.get(detail_url)).await?;
        ensure_model_scope_success(detail.success)?;

        let files_url = url_with_segments(
            &self.endpoints.model_scope_legacy_api,
            &["models", owner, name, "repo", "files"],
        )?;
        let file_envelope: ModelScopeFilesEnvelope = self
            .get_json(
                self.client
                    .get(files_url)
                    .query(&[("Revision", "master"), ("Recursive", "true")]),
            )
            .await?;
        if !file_envelope.success {
            return Err(RemoteModelCatalogError::InvalidResponse);
        }
        let mut files = file_envelope
            .data
            .files
            .into_iter()
            .take(MAX_REPOSITORY_FILES)
            .filter_map(remote_model_scope_file)
            .filter(|file| safe_remote_file(&file.path))
            .collect::<Vec<_>>();
        normalize_files(&mut files)?;
        let repository = format!("{owner}/{name}");
        Ok(RemoteModelRepository {
            source: DownloadSource::ModelScope,
            repository,
            display_name: nonempty(detail.data.display_name).unwrap_or_else(|| name.to_owned()),
            license: nonempty(detail.data.license),
            gated: detail.data.gated,
            private: detail.data.private,
            files,
        })
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, RemoteModelCatalogError> {
        let response = request
            .send()
            .await
            .map_err(|error| RemoteModelCatalogError::Network(network_error(&error)))?;
        if response.status() != StatusCode::OK {
            return Err(RemoteModelCatalogError::UpstreamStatus {
                status: response.status().as_u16(),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_CATALOG_RESPONSE_BYTES as u64)
        {
            return Err(RemoteModelCatalogError::ResponseTooLarge);
        }
        let mut body = BytesMut::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|error| RemoteModelCatalogError::Network(network_error(&error)))?;
            if body.len().saturating_add(chunk.len()) > MAX_CATALOG_RESPONSE_BYTES {
                return Err(RemoteModelCatalogError::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| RemoteModelCatalogError::InvalidResponse)
    }
}

fn parse_base_url(value: &str) -> Result<Url, RemoteModelCatalogError> {
    let url = Url::parse(value).map_err(|_| RemoteModelCatalogError::InvalidEndpoint)?;
    if !url.path().ends_with('/') {
        return Err(RemoteModelCatalogError::InvalidEndpoint);
    }
    Ok(url)
}

fn repository_url(
    base: &Url,
    collection: &str,
    owner: &str,
    name: &str,
) -> Result<Url, RemoteModelCatalogError> {
    url_with_segments(base, &[collection, owner, name])
}

fn url_with_segments(base: &Url, segments: &[&str]) -> Result<Url, RemoteModelCatalogError> {
    let mut url = base.clone();
    let mut path = url
        .path_segments_mut()
        .map_err(|_| RemoteModelCatalogError::InvalidEndpoint)?;
    path.pop_if_empty().extend(segments.iter().copied());
    drop(path);
    Ok(url)
}

fn validate_query(query: &str) -> Result<String, RemoteModelCatalogError> {
    let query = query.trim();
    if !(2..=100).contains(&query.chars().count()) || query.chars().any(char::is_control) {
        return Err(RemoteModelCatalogError::InvalidQuery);
    }
    Ok(query.to_owned())
}

fn validate_repository(repository: &str) -> Result<(&str, &str), RemoteModelCatalogError> {
    if repository.len() > 201 || repository.chars().any(char::is_control) {
        return Err(RemoteModelCatalogError::InvalidRepository);
    }
    let mut segments = repository.split('/');
    let owner = segments.next().unwrap_or_default();
    let name = segments.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || matches!(owner, "." | "..")
        || matches!(name, "." | "..")
        || segments.next().is_some()
        || !owner.chars().all(safe_repository_character)
        || !name.chars().all(safe_repository_character)
    {
        return Err(RemoteModelCatalogError::InvalidRepository);
    }
    Ok((owner, name))
}

fn safe_repository_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn safe_remote_file(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1_024
        && !path.starts_with('/')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn normalize_files(files: &mut Vec<RemoteGgufFile>) -> Result<(), RemoteModelCatalogError> {
    files.sort_by(|left, right| {
        left.size_bytes
            .cmp(&right.size_bytes)
            .then(left.path.cmp(&right.path))
    });
    files.dedup_by(|left, right| left.path == right.path);
    if files.is_empty() {
        Err(RemoteModelCatalogError::NoGgufFiles)
    } else {
        Ok(())
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty() && !value.eq_ignore_ascii_case("false"),
        _ => false,
    }
}

fn network_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "请求超时".to_owned()
    } else if error.is_connect() {
        "无法连接".to_owned()
    } else {
        "传输失败".to_owned()
    }
}

fn license_from_tags(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|tag| tag.strip_prefix("license:").map(str::to_owned))
}

fn license_from_hugging_face(
    tags: &[String],
    card_data: Option<&HuggingFaceCardData>,
) -> Option<String> {
    card_data
        .and_then(|card| nonempty(card.license.clone()))
        .or_else(|| license_from_tags(tags))
}

fn search_item_from_hugging_face(
    model: HuggingFaceModel,
) -> Result<RemoteModelSearchItem, RemoteModelCatalogError> {
    validate_repository(&model.id)?;
    Ok(RemoteModelSearchItem {
        source: DownloadSource::HuggingFace,
        display_name: model
            .id
            .split_once('/')
            .map(|(_, name)| name.to_owned())
            .unwrap_or_else(|| model.id.clone()),
        repository: model.id,
        license: license_from_hugging_face(&model.tags, model.card_data.as_ref()),
        downloads: model.downloads.unwrap_or(0),
        likes: model.likes.unwrap_or(0),
        parameter_count: None,
        repository_size_bytes: model.used_storage,
        gated: truthy(&model.gated),
        private: model.private,
    })
}

fn search_item_from_model_scope(
    model: ModelScopeModel,
) -> Result<RemoteModelSearchItem, RemoteModelCatalogError> {
    validate_repository(&model.id)?;
    Ok(RemoteModelSearchItem {
        source: DownloadSource::ModelScope,
        display_name: nonempty(model.display_name).unwrap_or_else(|| model.id.clone()),
        repository: model.id,
        license: nonempty(model.license),
        downloads: model.downloads.unwrap_or(0),
        likes: model.likes.unwrap_or(0),
        parameter_count: model.params,
        repository_size_bytes: model.file_size,
        gated: model.gated,
        private: model.private,
    })
}

fn remote_hugging_face_file(file: HuggingFaceFile, revision: &str) -> Option<RemoteGgufFile> {
    if !file.rfilename.to_ascii_lowercase().ends_with(".gguf") {
        return None;
    }
    let size_bytes = file
        .size
        .or_else(|| file.lfs.as_ref().and_then(|lfs| lfs.size))?;
    if size_bytes == 0 {
        return None;
    }
    Some(RemoteGgufFile {
        quantization: quantization_from_file_name(&file.rfilename),
        path: file.rfilename,
        size_bytes,
        sha256: file
            .lfs
            .and_then(|lfs| lfs.sha256)
            .filter(|value| valid_sha256(value)),
        revision: revision.to_owned(),
    })
}

fn remote_model_scope_file(file: ModelScopeFile) -> Option<RemoteGgufFile> {
    if !file.path.to_ascii_lowercase().ends_with(".gguf") || file.size == 0 {
        return None;
    }
    Some(RemoteGgufFile {
        quantization: quantization_from_file_name(&file.path),
        path: file.path,
        size_bytes: file.size,
        sha256: file.sha256.filter(|value| valid_sha256(value)),
        revision: file.revision,
    })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn ensure_model_scope_success(success: bool) -> Result<(), RemoteModelCatalogError> {
    if success {
        Ok(())
    } else {
        Err(RemoteModelCatalogError::InvalidResponse)
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HuggingFaceModel {
    id: String,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    likes: Option<u64>,
    #[serde(default)]
    gated: Value,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    card_data: Option<HuggingFaceCardData>,
    #[serde(default)]
    used_storage: Option<u64>,
    #[serde(default)]
    siblings: Vec<HuggingFaceFile>,
}

#[derive(Debug, serde::Deserialize)]
struct HuggingFaceCardData {
    #[serde(default)]
    license: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct HuggingFaceFile {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    lfs: Option<HuggingFaceLfs>,
}

#[derive(Debug, serde::Deserialize)]
struct HuggingFaceLfs {
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct ModelScopeSearchEnvelope {
    success: bool,
    data: ModelScopeSearchData,
}

#[derive(Debug, serde::Deserialize)]
struct ModelScopeSearchData {
    #[serde(default)]
    models: Vec<ModelScopeModel>,
}

#[derive(Debug, serde::Deserialize)]
struct ModelScopeDetailEnvelope {
    success: bool,
    data: ModelScopeModel,
}

#[derive(Debug, serde::Deserialize)]
struct ModelScopeModel {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    likes: Option<u64>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    params: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    gated: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelScopeFilesEnvelope {
    success: bool,
    data: ModelScopeFilesData,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelScopeFilesData {
    #[serde(default)]
    files: Vec<ModelScopeFile>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelScopeFile {
    path: String,
    size: u64,
    #[serde(default)]
    sha256: Option<String>,
    revision: String,
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn maps_both_official_catalog_shapes_into_one_contract() {
        let app = Router::new()
            .route("/hf/models", get(hf_search))
            .route("/hf/models/acme/model-gguf", get(hf_repository))
            .route("/ms-open/models", get(ms_search))
            .route("/ms-open/models/acme/model-gguf", get(ms_detail))
            .route(
                "/ms-legacy/models/acme/model-gguf/repo/files",
                get(ms_files),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener");
        let address = listener.local_addr().expect("listener address");
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        let catalog = RemoteModelCatalog::with_endpoints(
            &format!("http://{address}/hf/"),
            &format!("http://{address}/ms-open/"),
            &format!("http://{address}/ms-legacy/"),
        )
        .expect("catalog");

        let hf_results = catalog
            .search(DownloadSource::HuggingFace, "model gguf")
            .await
            .expect("HF search");
        assert_eq!(hf_results.items[0].repository, "acme/model-gguf");
        assert_eq!(hf_results.items[0].license.as_deref(), Some("mit"));
        let hf_repo = catalog
            .repository(DownloadSource::HuggingFace, "acme/model-gguf")
            .await
            .expect("HF repository");
        assert_eq!(hf_repo.files[0].quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(hf_repo.files[0].size_bytes, 2_048);

        let ms_results = catalog
            .search(DownloadSource::ModelScope, "model gguf")
            .await
            .expect("ModelScope search");
        assert_eq!(ms_results.items[0].repository, "acme/model-gguf");
        assert_eq!(ms_results.items[0].parameter_count, Some(600_000_000));
        let ms_repo = catalog
            .repository(DownloadSource::ModelScope, "acme/model-gguf")
            .await
            .expect("ModelScope repository");
        let expected_sha256 = "b".repeat(64);
        assert_eq!(
            ms_repo.files[0].sha256.as_deref(),
            Some(expected_sha256.as_str())
        );
        assert_eq!(ms_repo.files[0].revision, "commit-ms");

        task.abort();
    }

    #[test]
    fn rejects_unsafe_repository_and_file_paths() {
        assert!(validate_repository("owner/model").is_ok());
        assert!(validate_repository("https://example.test/model").is_err());
        assert!(validate_repository("owner/../model").is_err());
        assert!(safe_remote_file("weights/model.gguf"));
        assert!(!safe_remote_file("../model.gguf"));
        assert!(!safe_remote_file("/tmp/model.gguf"));
    }

    #[tokio::test]
    #[ignore = "calls the current official Hugging Face and ModelScope services"]
    async fn official_catalogs_resolve_a_public_gguf_repository() {
        let catalog = RemoteModelCatalog::new().expect("catalog");
        for source in [DownloadSource::HuggingFace, DownloadSource::ModelScope] {
            let repository = catalog
                .repository(source, "Qwen/Qwen3-0.6B-GGUF")
                .await
                .expect("official repository");
            assert!(!repository.files.is_empty());
            assert!(repository.files.iter().all(|file| file.size_bytes > 0));
        }
    }

    #[tokio::test]
    #[ignore = "calls Hugging Face to verify the pinned Qwen3.5-2B test artifact"]
    async fn hugging_face_resolves_pinned_qwen35_test_artifact() {
        let catalog = RemoteModelCatalog::new().expect("catalog");
        let repository = catalog
            .repository(DownloadSource::HuggingFace, "unsloth/Qwen3.5-2B-GGUF")
            .await
            .expect("Qwen3.5 GGUF repository");
        let file = repository
            .files
            .iter()
            .find(|file| file.path == "Qwen3.5-2B-Q4_K_M.gguf")
            .expect("Q4_K_M artifact");

        assert_eq!(repository.license.as_deref(), Some("apache-2.0"));
        assert_eq!(file.size_bytes, 1_280_835_840);
        assert_eq!(
            file.sha256.as_deref(),
            Some("aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223")
        );
        assert_eq!(file.revision, "f6d5376be1edb4d416d56da11e5397a961aca8ae");
    }

    async fn hf_search() -> Json<Value> {
        Json(json!([{
            "id": "acme/model-gguf",
            "downloads": 10,
            "likes": 2,
            "gated": false,
            "private": false,
            "tags": ["gguf", "license:mit"],
            "usedStorage": 2048
        }]))
    }

    async fn hf_repository() -> Json<Value> {
        Json(json!({
            "id": "acme/model-gguf",
            "sha": "commit-hf",
            "gated": false,
            "private": false,
            "tags": ["gguf", "license:mit"],
            "siblings": [{
                "rfilename": "model-Q4_K_M.gguf",
                "size": 2048,
                "lfs": {"sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 2048}
            }]
        }))
    }

    async fn ms_search() -> Json<Value> {
        Json(json!({
            "success": true,
            "data": {"models": [{
                "id": "acme/model-gguf",
                "display_name": "Model GGUF",
                "downloads": 20,
                "likes": 3,
                "license": "mit",
                "file_size": 4096,
                "params": 600000000,
                "tags": ["library:gguf"],
                "private": false,
                "gated": false
            }]}
        }))
    }

    async fn ms_detail() -> Json<Value> {
        Json(json!({
            "success": true,
            "data": {
                "id": "acme/model-gguf",
                "display_name": "Model GGUF",
                "license": "mit",
                "tags": ["library:gguf"],
                "private": false,
                "gated": false
            }
        }))
    }

    async fn ms_files() -> Json<Value> {
        Json(json!({
            "Success": true,
            "Data": {"Files": [{
                "Path": "model-Q5_K_M.gguf",
                "Size": 4096,
                "Sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "Revision": "commit-ms"
            }]}
        }))
    }
}
