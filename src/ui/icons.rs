use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;

use fontconfig_sys as fontconfig;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
pub use lucide_icons::Icon;
use simple_icons_pack::{
    Icon as BrandIcon, SI_ALIBABACLOUD, SI_ANTHROPIC, SI_CLOUDFLARE, SI_CURSOR, SI_DEEPSEEK,
    SI_GITHUBCOPILOT, SI_GITLAB, SI_GOOGLECLOUD, SI_GOOGLEGEMINI, SI_HUGGINGFACE, SI_KIMI,
    SI_METAAI, SI_MINIMAX, SI_MISTRALAI, SI_MOONSHOTAI, SI_NVIDIA, SI_OLLAMA, SI_OPENCODE,
    SI_OPENROUTER, SI_QWEN, SI_VERCEL, SI_VLLM, SI_X, SI_XIAOMI,
};

const OPENAI_ICON: BrandIcon = BrandIcon {
    svg: include_str!("../assets/openai.svg"),
    slug: "openai",
    title: "OpenAI",
    hex: "EEF0F4",
    source: "https://openai.com/brand",
    guidelines: None,
    license: None,
};

#[derive(Clone)]
pub struct ProviderIcon {
    pub root: gtk::Stack,
    brand: gtk::Image,
    generic: gtk::Label,
}

pub fn initialize_lucide_font() -> Result<(), String> {
    let path = std::env::temp_dir().join(format!(
        "omp-native-lucide-{}.ttf",
        env!("CARGO_PKG_VERSION")
    ));
    let bytes = lucide_icons::LUCIDE_FONT_BYTES;
    let needs_write = std::fs::metadata(&path)
        .map(|metadata| metadata.len() != bytes.len() as u64)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, bytes)
            .map_err(|error| format!("failed to stage Lucide font: {error}"))?;
    }
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Lucide font path contains a null byte".to_owned())?;
    unsafe {
        let config = fontconfig::FcConfigGetCurrent();
        if config.is_null() || fontconfig::FcConfigAppFontAddFile(config, path.as_ptr().cast()) == 0
        {
            return Err("fontconfig rejected the bundled Lucide font".to_owned());
        }
        if fontconfig::FcConfigBuildFonts(config) == 0 {
            return Err("fontconfig could not rebuild its application font set".to_owned());
        }
    }
    Ok(())
}

fn texture(bytes: &'static [u8], description: &str) -> gdk::Texture {
    gdk::Texture::from_bytes(&glib::Bytes::from_static(bytes))
        .unwrap_or_else(|error| panic!("{description} is not a valid SVG: {error}"))
}

pub fn icon(icon: Icon, size: i32) -> gtk::Label {
    let label = gtk::Label::new(Some(&char::from(icon).to_string()));
    let attributes = gtk::pango::AttrList::new();
    let mut font = gtk::pango::FontDescription::from_string("lucide");
    font.set_absolute_size(f64::from(size) * f64::from(gtk::pango::SCALE));
    attributes.insert(gtk::pango::AttrFontDesc::new(&font));
    label.set_attributes(Some(&attributes));
    label.set_accessible_role(gtk::AccessibleRole::Presentation);
    label.add_css_class("lucide-icon");
    label
}

pub fn icon_button(icon_name: Icon, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set_button_icon(&button, icon_name);
    button.set_tooltip_text(Some(tooltip));
    button
}

pub fn labeled_button(icon_name: Icon, text: &str) -> gtk::Button {
    let button = gtk::Button::new();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&icon(icon_name, 15));
    content.append(&gtk::Label::new(Some(text)));
    button.set_child(Some(&content));
    button
}

pub fn set_button_icon(button: &gtk::Button, icon_name: Icon) {
    button.set_child(Some(&icon(icon_name, 16)));
}

pub fn omp_logo(size: i32) -> gtk::Image {
    let image = gtk::Image::from_paintable(Some(&texture(
        include_bytes!("../assets/omp.svg"),
        "omp logo",
    )));
    image.set_pixel_size(size);
    image.add_css_class("brand-logo");
    image
}

pub fn provider_icon(provider: &str, size: i32) -> ProviderIcon {
    let root = gtk::Stack::new();
    root.set_size_request(size, size);
    let brand = gtk::Image::new();
    brand.set_pixel_size(size);
    let generic = gtk::Label::new(None);
    generic.add_css_class("provider-monogram");
    root.add_named(&brand, Some("brand"));
    root.add_named(&generic, Some("generic"));
    let provider_icon = ProviderIcon {
        root,
        brand,
        generic,
    };
    set_provider_icon(&provider_icon, provider);
    provider_icon
}

pub fn set_provider_icon(icon: &ProviderIcon, provider: &str) {
    if let Some(texture) = provider_texture(provider) {
        icon.brand.set_paintable(Some(&texture));
        icon.root.set_visible_child(&icon.brand);
    } else {
        icon.generic
            .set_text(&provider_monogram(&provider_label(provider)));
        icon.root.set_visible_child(&icon.generic);
    }
    icon.root.set_tooltip_text(Some(&provider_label(provider)));
}

thread_local! {
    static PROVIDER_TEXTURES: RefCell<HashMap<&'static str, gdk::Texture>> =
        RefCell::new(HashMap::new());
}

fn provider_texture(provider: &str) -> Option<gdk::Texture> {
    let asset = provider_brand_asset(provider)?;
    PROVIDER_TEXTURES.with(|cache| {
        if let Some(texture) = cache.borrow().get(asset.slug) {
            return Some(texture.clone());
        }
        let svg = asset.svg.replacen("<svg ", r##"<svg fill="#eef0f4" "##, 1);
        let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(svg.into_bytes()))
            .unwrap_or_else(|error| panic!("{} logo is not a valid SVG: {error}", asset.title));
        cache.borrow_mut().insert(asset.slug, texture.clone());
        Some(texture)
    })
}

fn provider_brand_asset(key: &str) -> Option<&'static BrandIcon> {
    let normalized = key.to_ascii_lowercase();
    let icon = match normalized.as_str() {
        value if value.contains("anthropic") || value.contains("claude") => &SI_ANTHROPIC,
        value if value.contains("openai") || value.contains("codex") => &OPENAI_ICON,
        "alibabacloud" | "alibaba-coding-plan" | "alibaba-token-plan" => &SI_ALIBABACLOUD,
        "cloudflare" | "cloudflare-ai-gateway" => &SI_CLOUDFLARE,
        "cursor" => &SI_CURSOR,
        "deepseek" => &SI_DEEPSEEK,
        "githubcopilot" | "github-copilot" => &SI_GITHUBCOPILOT,
        "gitlab" | "gitlab-duo" | "gitlab-duo-agent" => &SI_GITLAB,
        "googlecloud" | "google-vertex" => &SI_GOOGLECLOUD,
        "google" | "googlegemini" | "google-antigravity" | "google-gemini-cli" => &SI_GOOGLEGEMINI,
        "huggingface" => &SI_HUGGINGFACE,
        "kimi-code" => &SI_KIMI,
        "meta" | "metaai" => &SI_METAAI,
        "minimax" | "minimax-code" | "minimax-code-cn" => &SI_MINIMAX,
        "mistral" | "mistralai" => &SI_MISTRALAI,
        "nvidia" => &SI_NVIDIA,
        "moonshot" => &SI_MOONSHOTAI,
        "ollama" | "ollama-cloud" => &SI_OLLAMA,
        "opencode" | "opencode-go" | "opencode-zen" => &SI_OPENCODE,
        "openrouter" => &SI_OPENROUTER,
        "qwen-portal" => &SI_QWEN,
        "vercel" | "vercel-ai-gateway" => &SI_VERCEL,
        "vllm" => &SI_VLLM,
        "x" | "xai" | "xai-oauth" => &SI_X,
        "xiaomi" | "xiaomi-token-plan-ams" | "xiaomi-token-plan-cn" | "xiaomi-token-plan-sgp" => {
            &SI_XIAOMI
        }
        _ => return None,
    };
    Some(icon)
}

fn provider_monogram(label: &str) -> String {
    label
        .chars()
        .find(|character| character.is_alphanumeric())
        .map(|character| character.to_uppercase().collect())
        .unwrap_or_else(|| "AI".to_owned())
}

pub fn provider_label(provider: &str) -> String {
    let normalized = provider.to_ascii_lowercase();
    let label = match normalized.as_str() {
        "aiand" => "ai&",
        "aimlapi" => "AIML API",
        "alibaba-coding-plan" | "alibaba-token-plan" => "Alibaba Cloud",
        "amazon-bedrock" | "bedrock-mantle" | "aws" => "Amazon Bedrock",
        value if value.contains("anthropic") || value.contains("claude") => "Anthropic",
        "azure" => "Microsoft Azure",
        "baseten" => "Baseten",
        "cerebras" => "Cerebras",
        "cloudflare-ai-gateway" => "Cloudflare",
        "coreweave" => "CoreWeave",
        "cursor" => "Cursor",
        "deepseek" => "DeepSeek",
        "devin" => "Devin",
        "firepass" | "fireworks" => "Fireworks",
        "github-copilot" => "GitHub Copilot",
        "gitlab-duo" | "gitlab-duo-agent" => "GitLab",
        "gmi-cloud" => "GMI Cloud",
        "google" | "google-gemini-cli" => "Google",
        "google-antigravity" => "Google Antigravity",
        "google-vertex" => "Google Cloud",
        "groq" => "Groq",
        "huggingface" => "Hugging Face",
        "kilo" => "Kilo",
        "kimi-code" => "Kimi",
        "meta" => "Meta",
        "minimax" | "minimax-cn" | "minimax-code" | "minimax-code-cn" => "MiniMax",
        "mistral" => "Mistral",
        "moonshot" => "Moonshot AI",
        "nanogpt" => "NanoGPT",
        "novita" => "Novita",
        "nvidia" => "NVIDIA",
        "ollama" | "ollama-cloud" => "Ollama",
        value if value.contains("openai") || value.contains("codex") => "OpenAI",
        "opencode" | "opencode-go" | "opencode-zen" => "OpenCode",
        "openrouter" => "OpenRouter",
        "qianfan" => "Qianfan",
        "qwen-portal" => "Qwen",
        "sakana" => "Sakana AI",
        "synthetic" => "Synthetic",
        "together" => "Together AI",
        "umans" => "Umans AI",
        "venice" => "Venice",
        "vercel-ai-gateway" => "Vercel",
        "vllm" => "vLLM",
        "wafer-serverless" => "Wafer",
        "xai" | "xai-oauth" => "xAI",
        "xiaomi" | "xiaomi-token-plan-ams" | "xiaomi-token-plan-cn" | "xiaomi-token-plan-sgp" => {
            "Xiaomi"
        }
        "zai" => "Z.AI",
        "zenmux" => "ZenMux",
        "zhipu-coding-plan" => "Zhipu AI",
        _ => {
            return provider
                .split(['-', '_'])
                .filter(|part| !part.is_empty())
                .map(|part| {
                    let mut characters = part.chars();
                    characters.next().map_or_else(String::new, |first| {
                        first.to_uppercase().collect::<String>() + characters.as_str()
                    })
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    };
    label.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{provider_brand_asset, provider_label};

    #[test]
    fn provider_labels_hide_internal_adapter_names() {
        assert_eq!(provider_label("openai-codex"), "OpenAI");
        assert_eq!(provider_label("anthropic-oauth"), "Anthropic");
        assert_eq!(provider_label("amazon-bedrock"), "Amazon Bedrock");
        assert_eq!(provider_label("acme-local"), "Acme Local");
    }

    #[test]
    fn provider_icons_cover_available_ai_brands() {
        assert_eq!(provider_brand_asset("kimi-code").unwrap().title, "Kimi");
        assert_eq!(
            provider_brand_asset("moonshot").unwrap().title,
            "Moonshot AI"
        );
        assert_eq!(provider_brand_asset("qwen-portal").unwrap().title, "QWen");
        assert_eq!(provider_brand_asset("vllm").unwrap().title, "vLLM");
    }
}
