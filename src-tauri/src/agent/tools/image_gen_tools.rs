//! Image generation tool: `generate_image`
//!
//! Lets the AI agent generate images using AI models (local Ollama or
//! OpenAI-compatible APIs). Mirrors the structure of `create_svg`:
//!
//! - Validates the requested output path stays inside the workspace.
//! - Persists the bytes to disk.
//! - Triggers a `file-change` event so the sidebar refreshes and the
//!   in-app image viewer auto-opens the new file.
//! - Returns a structured `GenerateImageOutcome` carrying everything the
//!   chat panel needs to inline-preview without an extra `read_image`.
//!
//! ## Configuration
//!
//! Image-generation settings live alongside the other model configs in
//! `Settings::image_gen`. The struct is intentionally minimal: each entry
//! in `providers` is one backend (Ollama / OpenAI-compatible / etc.) and
//! `routing` decides which one is preferred when the LLM omits a `model`
//! override. Routing values:
//!
//! - `"local"`        → prefer the first enabled local provider (Ollama)
//! - `"cloud"`        → prefer the first enabled cloud provider
//! - anything else    → treated as a literal provider id
//!
//! If the user has not configured any image provider, the tool falls back
//! to Ollama on `localhost:11434` so the tool still does *something*
//! useful on a default install. We surface the fallback explicitly in the
//! log so the user can fix it via Settings if they meant to point at a
//! cloud API.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use super::super::super::settings_state::{
    get_image_gen_settings, ImageGenProviderConfig, ImageGenSettings,
};
use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

/// Maximum image size to accept (20 MB) when reading from a binary
/// source. Generators rarely produce images this large, but we cap
/// defensive against a buggy model.
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 4096;
const MAX_PROMPT_CHARS: usize = 16_000;

/// Structured outcome returned by `GenerateImageTool::execute`. Mirrors
/// `CreateSvgOutcome` so the registry / frontend can treat the two
/// consistently. Generated files already have a durable workspace path, so
/// downstream Office tools can embed them without copying their bytes into
/// the transient asset registry.
pub struct GenerateImageOutcome {
    pub output: String,
    pub file_path: String,
    pub byte_size: usize,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub model: String,
    pub is_error: bool,
}

#[derive(Debug, Deserialize)]
struct GenerateImageArgs {
    /// Natural-language description of the image to generate. Captured
    /// for the human-readable output line; the model itself reads the
    /// prompt verbatim — we don't expand prompts on the server.
    prompt: String,
    /// Absolute workspace-relative path the image should be written to.
    /// Extension picks the container: `.png`, `.jpg`/`.jpeg`, `.webp`.
    output_path: String,
    /// Optional width in pixels. Falls back to `ImageGenSettings.default_width`.
    #[serde(default)]
    width: Option<u32>,
    /// Optional height in pixels. Falls back to `ImageGenSettings.default_height`.
    #[serde(default)]
    height: Option<u32>,
    /// Retained for compatibility with older tool calls. A single output path
    /// can only represent one image, so values other than one are rejected.
    #[serde(default)]
    num_images: Option<u32>,
    /// Optional seed for reproducible results.
    #[serde(default)]
    seed: Option<u32>,
    /// Optional override. Either a bare model id (routed via
    /// `ImageGenSettings.routing`) or `<provider_id>/<model>` to force a
    /// specific backend (e.g. `"ollama/sdxl"` or `"openai/dall-e-3"`).
    #[serde(default)]
    model: Option<String>,
    /// Optional negative prompt (things to avoid). Only forwarded to
    /// backends that understand it (Stable Diffusion variants).
    #[serde(default)]
    negative_prompt: Option<String>,
}

pub struct GenerateImageTool;

impl GenerateImageTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "generate_image",
            "生成图片",
            "Generate an image using an AI image-generation model. The image \
             is written to the workspace and triggers a file-change event so \
             the in-app image viewer auto-opens it. \
             \
             Configuration is read from Settings → Image Generation, where you \
             can add one or more providers (Ollama, OpenAI-compatible, etc.) \
             and pick the default model + dimensions. The `model` parameter \
             here can either override the default model (then routing decides \
             which provider serves it) or pin a specific backend with the \
             `<provider_id>/<model>` syntax, e.g. `\"ollama/sdxl\"` or \
             `\"openai/dall-e-3\"`. \
             \
             Tips for prompts: be specific about subject, style (photorealistic, \
             watercolor, anime, oil painting), lighting, mood, composition, \
             and aspect ratio. The tool returns the absolute path of the \
             saved image — open it directly to inspect, or pass it to \
             `create_word_doc` / `create_pptx` to embed it in a document.",
            ToolParameters::new(
                vec!["prompt", "output_path"],
                vec![
                    ("prompt", "string", Some("Detailed natural-language description of the image to generate.")),
                    ("output_path", "string", Some("Absolute workspace path to save the image to. Must end with .png, .jpg, .jpeg, or .webp.")),
                    ("width", "integer", Some("Optional image width in pixels. Default: image_gen.default_width (1024).")),
                    ("height", "integer", Some("Optional image height in pixels. Default: image_gen.default_height (1024).")),
                    ("seed", "integer", Some("Optional seed for reproducible results.")),
                    ("model", "string", Some("Optional model override. Either a bare model id (routed via Settings → Image Generation) or `<provider_id>/<model>` to pin a specific backend.")),
                    ("negative_prompt", "string", Some("Optional negative prompt (things to avoid). Forwarded to backends that understand it.")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<GenerateImageOutcome, ToolError> {
        let mut args: GenerateImageArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidArguments(
                "generate_image".to_string(),
                format!("Invalid parameters: {}", e),
            )
        })?;

        args.prompt = args.prompt.trim().to_string();
        let prompt_chars = args.prompt.chars().count();
        if prompt_chars == 0 || prompt_chars > MAX_PROMPT_CHARS {
            return Err(ToolError::InvalidArguments(
                "generate_image".to_string(),
                format!("prompt must contain 1 to {} characters", MAX_PROMPT_CHARS),
            ));
        }
        if args.num_images.is_some_and(|count| count != 1) {
            return Err(ToolError::InvalidArguments(
                "generate_image".to_string(),
                "generate_image writes one output path and therefore supports exactly one image"
                    .to_string(),
            ));
        }

        // ── 1. Path validation ────────────────────────────────────────────
        let output_path = PathBuf::from(&args.output_path);
        let extension = output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let valid_extensions = ["png", "jpg", "jpeg", "webp"];
        if !valid_extensions.contains(&extension.as_str()) {
            return Err(ToolError::InvalidArguments(
                "generate_image".to_string(),
                format!(
                    "output_path must end with .png, .jpg, .jpeg, or .webp; got .{}",
                    extension
                ),
            ));
        }

        validate_workspace_path(&args.output_path, &workspace)?;

        // ── 2. Resolve settings + provider ─────────────────────────────────
        let settings = get_image_gen_settings();
        if !settings.enabled {
            return Err(ToolError::ExecutionError(
                "image generation is disabled in Settings → Image Generation".to_string(),
            ));
        }

        // Create parent directories if needed.
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::IoError(format!(
                        "Failed to create output directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        // Resolve which provider serves this call. The LLM can pin one
        // explicitly via `<provider_id>/<model>`; otherwise we honour
        // `settings.routing` and pick the first enabled match.
        let (provider, model_id) = resolve_provider(&settings, args.model.as_deref())
            .ok_or_else(|| {
                ToolError::ExecutionError(
                    "No enabled image generation provider matched. \
                     Configure one in Settings → Image Generation."
                        .to_string(),
                )
            })?;

        // Defaults fall through args → settings → 1024x1024 baked-in.
        let width = args.width.unwrap_or(settings.default_width);
        let height = args.height.unwrap_or(settings.default_height);
        if !(1..=MAX_IMAGE_DIMENSION).contains(&width)
            || !(1..=MAX_IMAGE_DIMENSION).contains(&height)
        {
            return Err(ToolError::InvalidArguments(
                "generate_image".to_string(),
                format!(
                    "width and height must be between 1 and {} pixels",
                    MAX_IMAGE_DIMENSION
                ),
            ));
        }

        // ── 3. Generate ────────────────────────────────────────────────────
        let (image_data, actual_model) = match provider.provider_type.as_str() {
            "ollama" => {
                self.generate_with_ollama(
                    provider,
                    &model_id,
                    &args.prompt,
                    width,
                    height,
                    args.seed,
                    args.negative_prompt.as_deref(),
                )
                .await?
            }
            "tencent_tc3" => {
                generate_with_tencent(
                    provider,
                    &model_id,
                    &args.prompt,
                    width,
                    height,
                )
                .await?
            }
            "tencent_token" => {
                generate_with_tencent_token(
                    provider,
                    &model_id,
                    &args.prompt,
                    width,
                    height,
                )
                .await?
            }
            // Treat everything else as OpenAI-compatible. That includes
            // "openai", "custom", plus any user-added provider whose
            // type we don't recognise — they all hit `/images/generations`.
            _ => {
                self.generate_with_openai_compatible(provider, &model_id, &args.prompt, width, height)
                    .await?
            }
        };

        if image_data.is_empty() {
            return Err(ToolError::ExecutionError(
                "image provider returned no images".to_string(),
            ));
        }

        let primary_image = image_data
            .into_iter()
            .next()
            .expect("checked non-empty image list");
        let byte_size = primary_image.len();

        if byte_size as u64 > MAX_IMAGE_BYTES {
            return Err(ToolError::ExecutionError(format!(
                "generated image is too large: {} bytes (limit {})",
                byte_size, MAX_IMAGE_BYTES
            )));
        }

        let encoded_extension = detect_encoded_image_extension(&primary_image).ok_or_else(|| {
            ToolError::ExecutionError(
                "image provider returned bytes in an unsupported or invalid image format"
                    .to_string(),
            )
        })?;
        let extension_matches = extension == encoded_extension
            || (extension == "jpeg" && encoded_extension == "jpg");
        if !extension_matches {
            return Err(ToolError::ExecutionError(format!(
                "image provider returned {} data, but output_path ends with .{}; use a matching extension",
                encoded_extension.to_uppercase(),
                extension
            )));
        }

        // ── 4. Write to workspace ──────────────────────────────────────────
        // Publish through a sibling temporary file. A crash or full disk must
        // not truncate an existing image that the document already embeds.
        let write_path = output_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::fs_utils::atomic_write(&write_path, &primary_image)
        })
        .await
        .map_err(|e| ToolError::IoError(format!("Image writer task failed: {}", e)))?
        .map_err(|e| {
            ToolError::IoError(format!(
                "Failed to write image to {}: {}",
                output_path.display(),
                e
            ))
        })?;

        // ── 5. Return outcome ─────────────────────────────────────────────
        let output = json!({
            "status": "ok",
            "file_path": output_path.to_string_lossy(),
            "prompt": args.prompt,
            "width": width,
            "height": height,
            "bytes": byte_size,
            "model": actual_model,
            "provider": provider.id,
        })
        .to_string();

        Ok(GenerateImageOutcome {
            output,
            file_path: output_path.to_string_lossy().to_string(),
            byte_size,
            width: Some(width),
            height: Some(height),
            model: actual_model,
            is_error: false,
        })
    }

    /// Generate image using Ollama's `/api/generate` endpoint. Ollama
    /// returns base64-encoded images in a top-level `images` array — see
    /// https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-completion.
    async fn generate_with_ollama(
        &self,
        provider: &ImageGenProviderConfig,
        model: &str,
        prompt: &str,
        width: u32,
        height: u32,
        seed: Option<u32>,
        negative_prompt: Option<&str>,
    ) -> Result<(Vec<Vec<u8>>, String), ToolError> {
        let base_url = provider
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:11434");
        let url = format!("{}/api/generate", base_url.trim_end_matches('/'));

        let mut request_body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.7,
            }
        });

        if let Some(opts) = request_body
            .get_mut("options")
            .and_then(|o| o.as_object_mut())
        {
            opts.insert("width".to_string(), serde_json::json!(width));
            opts.insert("height".to_string(), serde_json::json!(height));
            if let Some(seed_val) = seed {
                opts.insert("seed".to_string(), serde_json::json!(seed_val));
            }
        }

        if let Some(np) = negative_prompt {
            if let Some(root) = request_body.as_object_mut() {
                root.insert("negative_prompt".to_string(), serde_json::json!(np));
            }
        }

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                ToolError::ExecutionError(format!(
                    "Failed to connect to Ollama at {}: {}. \
                     Make sure Ollama is running with `ollama serve`",
                    base_url, e
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionError(format!(
                "Ollama image generation failed (HTTP {}): {}",
                status, error_text
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse Ollama response: {}", e)))?;

        let images = response_json["images"]
            .as_array()
            .ok_or_else(|| {
                ToolError::ExecutionError(
                    "Ollama response missing 'images' field. \
                     Is the configured model actually an image-generation model?"
                        .to_string(),
                )
            })?;

        let mut image_data: Vec<Vec<u8>> = Vec::new();
        for img in images {
            if let Some(b64_str) = img.as_str() {
                let bytes = BASE64.decode(b64_str).map_err(|e| {
                    ToolError::ExecutionError(format!("Failed to decode base64 image: {}", e))
                })?;
                image_data.push(bytes);
            }
        }

        if image_data.is_empty() {
            return Err(ToolError::ExecutionError(
                "Ollama response contained no decodable images".to_string(),
            ));
        }

        Ok((image_data, model.to_string()))
    }

    /// Generate image using OpenAI-compatible `/v1/images/generations`
    /// endpoint. Works against any provider that mimics the OpenAI Image
    /// API (OpenAI proper, inkuo Cloud's image routing, OpenRouter's
    /// image models, etc.).
    async fn generate_with_openai_compatible(
        &self,
        provider: &ImageGenProviderConfig,
        model: &str,
        prompt: &str,
        width: u32,
        height: u32,
    ) -> Result<(Vec<Vec<u8>>, String), ToolError> {
        let base_url = provider
            .base_url
            .as_deref()
            .ok_or_else(|| {
                ToolError::ExecutionError(format!(
                    "image provider '{}' has no base_url configured",
                    provider.id
                ))
            })?;
        let api_key = provider.api_key.as_deref().ok_or_else(|| {
            ToolError::ExecutionError(format!(
                "image provider '{}' has no API key configured",
                provider.id
            ))
        })?;

        let url = format!("{}/images/generations", base_url.trim_end_matches('/'));

        // Map common model aliases so users can pass "sdxl" / "dall-e-3"
        // without remembering the exact upstream id.
        let model_name = match model.to_lowercase().as_str() {
            "dalle3" | "dall-e-3" => "dall-e-3",
            "dalle2" | "dall-e-2" => "dall-e-2",
            "sdxl" | "stable-diffusion" | "sd" => "sdxl",
            _ => model,
        };

        let client = reqwest::Client::new();
        let request_body = serde_json::json!({
            "model": model_name,
            "prompt": prompt,
            "n": 1,
            "size": format!("{}x{}", width, height),
            "response_format": "b64_json",
        });

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                ToolError::ExecutionError(format!(
                    "Failed to connect to image API at {}: {}",
                    base_url, e
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionError(format!(
                "Image API at {} failed (HTTP {}): {}. \
                 Check the API key and model configured for provider '{}'.",
                base_url, status, error_text, provider.id
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse API response: {}", e)))?;

        let images = response_json["data"]
            .as_array()
            .ok_or_else(|| {
                ToolError::ExecutionError(
                    "image API response missing 'data' array".to_string(),
                )
            })?;

        let mut image_data: Vec<Vec<u8>> = Vec::new();
        for img in images {
            if let Some(b64_str) = img["b64_json"].as_str() {
                let bytes = BASE64.decode(b64_str).map_err(|e| {
                    ToolError::ExecutionError(format!("Failed to decode base64 image: {}", e))
                })?;
                image_data.push(bytes);
            }
        }

        if image_data.is_empty() {
            return Err(ToolError::ExecutionError(
                "image API response contained no decodable images".to_string(),
            ));
        }

        Ok((image_data, model_name.to_string()))
    }
}

fn detect_encoded_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn validate_generated_image_url(raw: &str) -> Result<reqwest::Url, ToolError> {
    let url = reqwest::Url::parse(raw).map_err(|_| {
        ToolError::ExecutionError("image provider returned an invalid download URL".to_string())
    })?;
    if url.scheme() != "https" {
        return Err(ToolError::ExecutionError(
            "image provider download URL must use HTTPS".to_string(),
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        ToolError::ExecutionError("image provider download URL has no host".to_string())
    })?;
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return Err(ToolError::ExecutionError(
            "image provider download URL cannot target localhost".to_string(),
        ));
    }
    let ip_literal = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    if ip_literal.parse::<IpAddr>().is_ok_and(is_non_public_ip) {
        return Err(ToolError::ExecutionError(
            "image provider download URL cannot target a private or reserved address".to_string(),
        ));
    }
    Ok(url)
}

fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && (18..=19).contains(&octets[1]))
                || octets[0] >= 240
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.segments()[..2] == [0x2001, 0x0db8]
                || ip.to_ipv4_mapped().is_some_and(|mapped| {
                    is_non_public_ip(IpAddr::V4(mapped))
                })
        }
    }
}

async fn download_generated_image(raw_url: &str, provider_name: &str) -> Result<Vec<u8>, ToolError> {
    let url = validate_generated_image_url(raw_url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        // Never follow an otherwise-public URL to a local metadata service.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ToolError::ExecutionError(format!("Failed to initialize image downloader: {}", e)))?;
    let mut response = client.get(url).send().await.map_err(|e| {
        ToolError::ExecutionError(format!(
            "Failed to download generated image from {}: {}",
            provider_name, e
        ))
    })?;
    if !response.status().is_success() {
        return Err(ToolError::ExecutionError(format!(
            "{} image download failed with HTTP {}",
            provider_name,
            response.status()
        )));
    }
    if response.content_length().is_some_and(|length| length > MAX_IMAGE_BYTES) {
        return Err(ToolError::ExecutionError(format!(
            "{} image download exceeds the {} byte limit",
            provider_name, MAX_IMAGE_BYTES
        )));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        ToolError::ExecutionError(format!(
            "Failed to read generated image from {}: {}",
            provider_name, e
        ))
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_IMAGE_BYTES as usize {
            return Err(ToolError::ExecutionError(format!(
                "{} image download exceeds the {} byte limit",
                provider_name, MAX_IMAGE_BYTES
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod encoded_image_tests {
    use super::{detect_encoded_image_extension, validate_generated_image_url};

    #[test]
    fn recognises_supported_image_signatures() {
        assert_eq!(
            detect_encoded_image_extension(b"\x89PNG\r\n\x1a\nrest"),
            Some("png")
        );
        assert_eq!(
            detect_encoded_image_extension(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("jpg")
        );
        assert_eq!(
            detect_encoded_image_extension(b"RIFF\x04\x00\x00\x00WEBP"),
            Some("webp")
        );
    }

    #[test]
    fn rejects_unknown_or_truncated_payloads() {
        assert_eq!(detect_encoded_image_extension(b"not-an-image"), None);
        assert_eq!(detect_encoded_image_extension(b"RIFF"), None);
    }

    #[test]
    fn generated_image_urls_must_be_public_https_targets() {
        assert!(validate_generated_image_url("https://cdn.example.com/image.png").is_ok());
        for blocked in [
            "http://cdn.example.com/image.png",
            "https://localhost/image.png",
            "https://127.0.0.1/image.png",
            "https://10.0.0.5/image.png",
            "https://[::1]/image.png",
            "https://[::ffff:127.0.0.1]/image.png",
        ] {
            assert!(
                validate_generated_image_url(blocked).is_err(),
                "accepted {blocked}"
            );
        }
    }
}

/// Pick the provider + model id that should serve this call. The LLM can
/// either pin a backend with `<provider_id>/<model>` or pass a bare
/// model id and let `settings.routing` decide.
fn resolve_provider<'a>(
    settings: &'a ImageGenSettings,
    model_arg: Option<&str>,
) -> Option<(&'a ImageGenProviderConfig, String)> {
    let enabled: Vec<&ImageGenProviderConfig> = settings
        .providers
        .iter()
        .filter(|p| p.enabled)
        .collect();

    if enabled.is_empty() {
        return None;
    }

    // Explicit pinning: "<provider_id>/<model>". The provider id is the
    // stable opaque identifier (e.g. "ollama-default", or any user-added
    // uuid). The model portion is forwarded verbatim — we don't validate
    // model ids at this layer; the upstream API will reject bad ones.
    if let Some(raw) = model_arg {
        if let Some((id, model)) = raw.split_once('/') {
            if let Some(p) = enabled.iter().find(|p| p.id == id) {
                return Some((p, model.to_string()));
            }
        }
        // Bare model id: route by `settings.routing`.
    }

    let routed = match settings.routing.as_str() {
        // "local" prefers an Ollama provider, falling back to the first
        // enabled entry (the upstream call will fail informatively rather
        // than silently succeeding on a remote endpoint).
        "local" => enabled
            .iter()
            .find(|p| p.provider_type == "ollama")
            .or_else(|| enabled.first())
            .copied(),
        // "cloud" prefers any non-Ollama provider. We treat anything
        // that isn't explicitly `"ollama"` as cloud — that includes
        // `"openai"`, `"tencent_token"`, `"tencent_tc3"`, `"custom"`,
        // and any future additions.
        "cloud" => enabled
            .iter()
            .find(|p| p.provider_type != "ollama")
            .or_else(|| enabled.first())
            .copied(),
        // Explicit routing to a named provider id — useful when the user
        // has multiple remote endpoints and wants one of them by default.
        other => enabled
            .iter()
            .find(|p| p.id == other)
            .or_else(|| enabled.first())
            .copied(),
    }?;

    let model_id = model_arg
        .map(str::to_string)
        .unwrap_or_else(|| routed.default_model.clone());
    Some((routed, model_id))
}

impl Default for GenerateImageTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for GenerateImageOutcome {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("GenerateImageOutcome", 6)?;
        st.serialize_field("output", &self.output)?;
        st.serialize_field("file_path", &self.file_path)?;
        st.serialize_field("byte_size", &self.byte_size)?;
        st.serialize_field("width", &self.width)?;
        st.serialize_field("height", &self.height)?;
        st.serialize_field("model", &self.model)?;
        st.serialize_field("is_error", &self.is_error)?;
        st.end()
    }
}

// ============================================================================
// Tencent Cloud TC3-HMAC-SHA256 signing
// ============================================================================
//
// Tencent Cloud APIs authenticate every request with a derived signature,
// not a Bearer token. The algorithm is documented at
// https://cloud.tencent.com/document/api/1729/101843. In short:
//
//   1. Build the canonical request:
//        HTTPMethod\nCanonicalURI\nCanonicalQueryString\nCanonicalHeaders\nSignedHeaders\nHashedRequestPayload
//   2. Build the string-to-sign:
//        TC3-HMAC-SHA256\nRequestTimestamp\nCredentialScope\nHashedCanonicalRequest
//   3. Derive the signing key from the secret key + scope:
//        SecretDate = HMAC-SHA256("TC3" + SecretKey, Date)
//        SecretService = HMAC-SHA256(SecretDate, Service)
//        SecretSigning = HMAC-SHA256(SecretService, "tc3_request")
//   4. Compute the signature:
//        Signature = HMAC-SHA256(SecretSigning, StringToSign).hex_lower()
//
// The signed headers are then attached to the request as
// `Authorization: TC3-HMAC-SHA256 Credential=... Signature=...`.

/// All the inputs needed to sign a single Tencent Cloud API request.
///
/// Lives next to `GenerateImageTool` so the implementation stays in one
/// file; the test path also reuses it.
struct TencentRequest<'a> {
    host: &'a str,
    service: &'a str,
    payload: &'a str,
    /// Unix timestamp (seconds) the signature is anchored to. Caller
    /// chooses it so test code can pin a deterministic value.
    timestamp: i64,
    /// RFC3339-style date used in the credential scope (e.g. `2024-01-15`).
    date: &'a str,
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::Mac;
    let mut mac = <hmac::Hmac<sha2::Sha256> as hmac::Mac>::new_from_slice(key)
        .expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Build the canonical request string.
///
/// `payload_hash` is the lowercase hex SHA-256 of the JSON body. Pass
/// the precomputed hash instead of the raw bytes so the wrapper can
/// short-circuit when the caller already hashed the body once.
fn tencent_canonical_request(
    req: &TencentRequest,
    payload_hash: &str,
) -> (String, String) {
    let canonical_uri = "/".to_string();
    let canonical_query = "".to_string();
    let canonical_headers = format!(
        "content-type:application/json\nhost:{}\n",
        req.host
    );
    let signed_headers = "content-type;host".to_string();
    let canonical = format!(
        "POST\n{}\n{}\n{}\n{}\n{}",
        canonical_uri,
        canonical_query,
        canonical_headers,
        signed_headers,
        payload_hash
    );
    (canonical, signed_headers)
}

/// Compute the lowercase hex signature for a Tencent Cloud API request.
///
/// Returns the full `Authorization` header value ready to attach to the
/// outbound HTTP request. The public flat-field wrapper below is used by the
/// settings test-connection path.
fn tencent_authorization(
    secret_id: &str,
    secret_key: &str,
    req: &TencentRequest,
) -> Result<String, String> {
    let payload_hash = sha256_hex(req.payload.as_bytes());
    let (canonical, signed_headers) = tencent_canonical_request(req, &payload_hash);

    let credential_scope = format!("{}/{}/tc3_request", req.date, req.service);
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{}\n{}\n{}",
        req.timestamp,
        credential_scope,
        sha256_hex(canonical.as_bytes())
    );

    // Step 3: derive the signing key by chaining HMAC-SHA256 calls.
    let secret_date = hmac_sha256(
        format!("TC3{}", secret_key).as_bytes(),
        req.date.as_bytes(),
    );
    let secret_service = hmac_sha256(&secret_date, req.service.as_bytes());
    let secret_signing = hmac_sha256(&secret_service, b"tc3_request");

    let signature = hex::encode(hmac_sha256(
        &secret_signing,
        string_to_sign.as_bytes(),
    ));

    Ok(format!(
        "TC3-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        secret_id, credential_scope, signed_headers, signature
    ))
}

/// Convenience wrapper around `tencent_authorization` that builds the
/// `TencentRequest` from flat fields. Exposed at `pub` level so callers
/// outside `image_gen_tools` (the AI test path) can sign without
/// reaching into the struct.
pub fn sign_tencent_request(
    secret_id: &str,
    secret_key: &str,
    host: &str,
    service: &str,
    _action: &str,
    _region: &str,
    payload: &str,
    timestamp: i64,
    date: &str,
) -> Result<String, String> {
    let req = TencentRequest {
        host,
        service,
        payload,
        timestamp,
        date,
    };
    tencent_authorization(secret_id, secret_key, &req)
}

/// Send a signed request to a Tencent Cloud API. Wraps the response
/// body into a `serde_json::Value` so callers can pluck out the
/// `Response.ResultImage` (or similar) field without re-parsing the
/// raw bytes.
async fn send_tencent_request(
    host: &str,
    action: &str,
    payload: &str,
    provider: &ImageGenProviderConfig,
) -> Result<serde_json::Value, ToolError> {
    let secret_id = provider.secret_id.as_deref().ok_or_else(|| {
        ToolError::ExecutionError(format!(
            "Tencent provider '{}' is missing SecretId — \
             required for TC3 signing",
            provider.id
        ))
    })?;
    let secret_key = provider.secret_key.as_deref().ok_or_else(|| {
        ToolError::ExecutionError(format!(
            "Tencent provider '{}' is missing SecretKey — \
             required for TC3 signing",
            provider.id
        ))
    })?;

    let now = chrono::Utc::now().timestamp();
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let region = provider.region.as_deref().unwrap_or("ap-guangzhou");
    let sign_req = TencentRequest {
        host,
        service: "aiart",
        payload,
        timestamp: now,
        date: &date,
    };

    let authorization = tencent_authorization(secret_id, secret_key, &sign_req)
        .map_err(ToolError::ExecutionError)?;

    let url = format!("https://{}", host);
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", authorization)
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Host", host)
        .header("X-TC-Action", action)
        .header("X-TC-Timestamp", now.to_string())
        .header("X-TC-Version", "2023-09-01")
        .header("X-TC-Region", region)
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| {
            ToolError::ExecutionError(format!(
                "Failed to connect to Tencent Cloud at {}: {}",
                host, e
            ))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionError(format!(
            "Tencent Cloud API returned HTTP {}: {}. \
             Check that SecretId/SecretKey belong to the same Tencent account \
             and that '{}' is enabled.",
            status, body, action
        )));
    }

    response.json::<serde_json::Value>().await.map_err(|e| {
        ToolError::ExecutionError(format!(
            "Tencent Cloud returned a non-JSON response: {}",
            e
        ))
    })
}

/// Tencent Cloud Hunyuan image generation. The `aiart` service exposes
/// `TextToImageLite` (low-quality, fast) and `TextToImagePro` (high-
/// quality). The default model the user typed in the settings panel is
/// used directly — we don't auto-route to Lite vs Pro so the user keeps
/// full control over cost vs quality.
async fn generate_with_tencent(
    provider: &ImageGenProviderConfig,
    model: &str,
    prompt: &str,
    width: u32,
    height: u32,
) -> Result<(Vec<Vec<u8>>, String), ToolError> {
    let request_body = serde_json::json!({
        "Prompt": prompt,
        "RspImgType": "url",
        "Width": width,
        "Height": height,
        // Model is forwarded as-is so the user can pick the exact tencent
        // model id (e.g. "hunyuan-pro", "hunyuan-lite", etc.) without us
        // hard-coding an alias table.
        "Model": model,
    })
    .to_string();

    let response = send_tencent_request(
        "aiart.tencentcloudapi.com",
        "TextToImageLite",
        &request_body,
        provider,
    )
    .await?;

    let result_image = response
        .pointer("/Response/ResultImage")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolError::ExecutionError(format!(
                "Tencent Cloud response missing 'Response.ResultImage': {}",
                response
            ))
        })?;

    let image_bytes = download_generated_image(result_image, "Tencent Cloud").await?;

    Ok((vec![image_bytes], model.to_string()))
}

/// Tencent Token Hub (tokenhub.tencentmaas.com). OpenAI-compatible wire
/// format with a single Bearer API key. The API path is
/// `POST /v1/api/image/lite` and the body uses `rsp_img_type` (snake_case)
/// instead of OpenAI's `response_format`.
async fn generate_with_tencent_token(
    provider: &ImageGenProviderConfig,
    model: &str,
    prompt: &str,
    _width: u32,
    _height: u32,
) -> Result<(Vec<Vec<u8>>, String), ToolError> {
    let base_url = provider
        .base_url
        .as_deref()
        .ok_or_else(|| {
            ToolError::ExecutionError(format!(
                "Tencent Token Hub provider '{}' has no base_url configured",
                provider.id
            ))
        })?;
    let api_key = provider.api_key.as_deref().ok_or_else(|| {
        ToolError::ExecutionError(format!(
            "Tencent Token Hub provider '{}' has no API key configured",
            provider.id
        ))
    })?;

    // The API path differs from the standard OpenAI `/v1/images/generations`.
    let url = format!("{}/v1/api/image/lite", base_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    // Tencent Token Hub uses snake_case field names and `rsp_img_type`
    // instead of `response_format`.
    let request_body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "rsp_img_type": "url",
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            ToolError::ExecutionError(format!(
                "Failed to connect to Tencent Token Hub at {}: {}",
                base_url, e
            ))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionError(format!(
            "Tencent Token Hub at {} failed (HTTP {}): {}. \
             Check the API key and model configured for provider '{}'.",
            base_url, status, error_text, provider.id
        )));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| {
            ToolError::ExecutionError(format!(
                "Tencent Token Hub returned a non-JSON response: {}",
                e
            ))
        })?;

    // The response shape is: { "image_url": "https://..." } or
    // { "data": [{ "url": "..." }] }. Handle both.
    let image_url = response_json
        .pointer("/image_url")
        .and_then(|v| v.as_str())
        .or_else(|| {
            response_json
                .pointer("/data/0/url")
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| {
            ToolError::ExecutionError(format!(
                "Tencent Token Hub response missing 'image_url' or \
                 'data[0].url' field: {}",
                response_json
            ))
        })?;

    let image_bytes = download_generated_image(image_url, "Tencent Token Hub").await?;

    Ok((vec![image_bytes], model.to_string()))
}
