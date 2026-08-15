use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;

use fontconfig_sys as fontconfig;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
pub use lucide_icons::Icon;

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
    let generic = icon(Icon::Cpu, size);
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
    let normalized = provider.to_ascii_lowercase();
    let bytes = if normalized.contains("anthropic") || normalized.contains("claude") {
        Some((
            include_bytes!("../assets/anthropic.svg").as_slice(),
            "Anthropic logo",
        ))
    } else if normalized.contains("openai") || normalized.contains("codex") {
        Some((
            include_bytes!("../assets/openai.svg").as_slice(),
            "OpenAI logo",
        ))
    } else {
        None
    };

    if let Some((bytes, description)) = bytes {
        icon.brand.set_paintable(Some(&texture(bytes, description)));
        icon.root.set_visible_child(&icon.brand);
    } else {
        icon.root.set_visible_child(&icon.generic);
    }
    icon.root.set_tooltip_text(Some(&provider_label(provider)));
}

pub fn provider_label(provider: &str) -> String {
    let normalized = provider.to_ascii_lowercase();
    if normalized.contains("anthropic") || normalized.contains("claude") {
        "Anthropic".to_owned()
    } else if normalized.contains("openai") || normalized.contains("codex") {
        "OpenAI".to_owned()
    } else if normalized.contains("google") || normalized.contains("gemini") {
        "Google".to_owned()
    } else if normalized.contains("mistral") {
        "Mistral".to_owned()
    } else if normalized.contains("xai") || normalized.contains("grok") {
        "xAI".to_owned()
    } else if normalized.contains("bedrock") || normalized == "aws" {
        "Amazon Bedrock".to_owned()
    } else if normalized.contains("openrouter") {
        "OpenRouter".to_owned()
    } else if normalized.contains("github") || normalized.contains("copilot") {
        "GitHub Copilot".to_owned()
    } else if normalized.contains("ollama") {
        "Ollama".to_owned()
    } else if normalized.contains("llama") {
        "llama.cpp".to_owned()
    } else {
        provider
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut characters = part.chars();
                characters.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + characters.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::provider_label;

    #[test]
    fn provider_labels_hide_internal_adapter_names() {
        assert_eq!(provider_label("openai-codex"), "OpenAI");
        assert_eq!(provider_label("anthropic-oauth"), "Anthropic");
        assert_eq!(provider_label("amazon-bedrock"), "Amazon Bedrock");
        assert_eq!(provider_label("acme-local"), "Acme Local");
    }
}
