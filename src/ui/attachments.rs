use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::bridge::protocol::ImageContent;

pub(crate) type AttachmentId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttachmentMeta {
    id: AttachmentId,
    name: String,
}

#[derive(Debug, Default)]
pub(crate) struct ComposerAttachments {
    next_id: AttachmentId,
    metadata: Vec<AttachmentMeta>,
    images: Vec<ImageContent>,
}

impl ComposerAttachments {
    pub(crate) fn add(&mut self, name: impl Into<String>, image: ImageContent) -> AttachmentId {
        let id = self.next_id.checked_add(1).expect("attachment id overflow");
        self.next_id = id;
        self.metadata.push(AttachmentMeta {
            id,
            name: name.into(),
        });
        self.images.push(image);
        id
    }

    pub(crate) fn remove(&mut self, id: AttachmentId) -> bool {
        let Some(index) = self.metadata.iter().position(|item| item.id == id) else {
            return false;
        };
        self.metadata.remove(index);
        self.images.remove(index);
        true
    }

    pub(crate) fn resolve_submission(&mut self, ids: &[AttachmentId], accepted: bool) {
        if !accepted {
            return;
        }
        let mut index = 0;
        while index < self.metadata.len() {
            if ids.contains(&self.metadata[index].id) {
                self.metadata.remove(index);
                self.images.remove(index);
            } else {
                index += 1;
            }
        }
    }

    pub(crate) fn summaries(&self) -> impl Iterator<Item = (AttachmentId, &str)> {
        self.metadata
            .iter()
            .map(|item| (item.id, item.name.as_str()))
    }

    pub(crate) fn ids(&self) -> impl Iterator<Item = AttachmentId> + '_ {
        self.metadata.iter().map(|item| item.id)
    }

    pub(crate) fn images(&self) -> &[ImageContent] {
        &self.images
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.metadata.is_empty()
    }
}

pub(crate) fn encode_image(bytes: &[u8]) -> Result<ImageContent, String> {
    let mime_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return Err("Only PNG and JPEG images can be attached".to_owned());
    };
    Ok(ImageContent::new(STANDARD.encode(bytes), mime_type))
}

#[cfg(test)]
mod tests {
    use super::{ComposerAttachments, encode_image};
    use crate::bridge::protocol::ImageContent;

    fn image(data: &str) -> ImageContent {
        ImageContent::new(data.to_owned(), "image/png")
    }

    #[test]
    fn removal_preserves_the_order_of_every_other_image() {
        let mut attachments = ComposerAttachments::default();
        let first = attachments.add("first.png", image("first"));
        let middle = attachments.add("middle.png", image("middle"));
        let last = attachments.add("last.png", image("last"));

        assert!(attachments.remove(middle));
        assert_eq!(
            attachments.ids().collect::<Vec<_>>(),
            vec![first, last]
        );
        assert_eq!(
            attachments
                .images()
                .iter()
                .map(|image| image.data.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "last"]
        );
    }

    #[test]
    fn rejected_submission_retains_images_and_acceptance_removes_only_its_snapshot() {
        let mut attachments = ComposerAttachments::default();
        let first = attachments.add("first.png", image("first"));
        let second = attachments.add("second.png", image("second"));
        let submitted = [first, second];
        let later = attachments.add("later.png", image("later"));

        attachments.resolve_submission(&submitted, false);
        assert_eq!(
            attachments.ids().collect::<Vec<_>>(),
            vec![first, second, later]
        );

        attachments.resolve_submission(&submitted, true);
        assert_eq!(attachments.ids().collect::<Vec<_>>(), vec![later]);
        assert_eq!(attachments.images()[0].data, "later");
    }

    #[test]
    fn image_encoding_accepts_only_png_and_jpeg_signatures() {
        let png = encode_image(b"\x89PNG\r\n\x1a\npayload").expect("encode png");
        let jpeg = encode_image(b"\xff\xd8\xffpayload").expect("encode jpeg");
        assert_eq!(png.mime_type, "image/png");
        assert_eq!(jpeg.mime_type, "image/jpeg");
        assert!(encode_image(b"GIF89a").is_err());
    }
}
